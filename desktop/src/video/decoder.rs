// FFmpeg-based video decoder for H.264, H.265/HEVC, and MJPEG streams.
//
// Receives encoded NAL units (or JPEG frames), decodes them to raw video,
// and converts the output to YUYV422 format for v4l2loopback.

use anyhow::{bail, Context, Result};
use crossbeam_channel::{Receiver, Sender};
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

extern crate ffmpeg_next as ffmpeg;

use ffmpeg::codec;
use ffmpeg::software::scaling;

/// Supported video codecs that the decoder can handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Mjpeg,
}

impl VideoCodec {
    /// Parse a codec name string into the enum variant.
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "h264" | "avc" => Ok(VideoCodec::H264),
            "h265" | "hevc" => Ok(VideoCodec::H265),
            "mjpeg" | "jpeg" => Ok(VideoCodec::Mjpeg),
            _ => bail!("Unsupported codec: {}", s),
        }
    }

    /// Get the FFmpeg codec ID for this video codec.
    fn ffmpeg_id(&self) -> codec::Id {
        match self {
            VideoCodec::H264 => codec::Id::H264,
            VideoCodec::H265 => codec::Id::HEVC,
            VideoCodec::Mjpeg => codec::Id::MJPEG,
        }
    }
}

/// Statistics tracked by the decoder for monitoring.
#[derive(Debug, Default)]
pub struct DecoderStats {
    pub frames_decoded: u64,
    pub frames_dropped: u64,
    pub decode_errors: u64,
}

/// Spawn a decoder thread that reads encoded frames, decodes them, and outputs
/// raw YUYV422 frames suitable for v4l2loopback.
///
/// # Arguments
/// * `video_codec` - The codec to decode
/// * `video_config_rx` - Channel receiving codec config data (SPS/PPS/VPS)
/// * `video_rx` - Channel receiving encoded video frames
/// * `frame_tx` - Channel to send decoded YUYV422 frames
/// * `width` - Expected output width
/// * `height` - Expected output height
/// * `shutdown` - Shutdown signal
pub fn spawn_decoder_thread(
    video_codec: VideoCodec,
    video_config_rx: Receiver<Vec<u8>>,
    video_rx: Receiver<Vec<u8>>,
    frame_tx: Sender<Vec<u8>>,
    width: u32,
    height: u32,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::Builder::new()
        .name("video-decoder".into())
        .spawn(move || {
            if let Err(e) = decoder_loop(
                video_codec,
                video_config_rx,
                video_rx,
                frame_tx,
                width,
                height,
                shutdown,
            ) {
                error!("Video decoder error: {:#}", e);
            }
            info!("Video decoder thread exiting");
        })
        .context("Failed to spawn video decoder thread")?;

    Ok(handle)
}

/// Main decoder loop — initializes FFmpeg, decodes packets, scales output.
fn decoder_loop(
    video_codec: VideoCodec,
    video_config_rx: Receiver<Vec<u8>>,
    video_rx: Receiver<Vec<u8>>,
    frame_tx: Sender<Vec<u8>>,
    width: u32,
    height: u32,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    info!(
        "Video decoder starting: {:?} {}x{}",
        video_codec, width, height
    );

    // Find the appropriate FFmpeg decoder
    let codec = codec::decoder::find(video_codec.ffmpeg_id())
        .with_context(|| format!("FFmpeg decoder not found for {:?}", video_codec))?;

    let mut context = codec::Context::new_with_codec(codec);

    // For MJPEG, we can start decoding immediately since each frame is independent.
    // For H.264/H.265, we need the codec configuration (SPS/PPS) first.
    if video_codec != VideoCodec::Mjpeg {
        info!("Waiting for video codec configuration (SPS/PPS)...");

        // Wait for the config data with a timeout
        match video_config_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(config_data) => {
                info!(
                    "Received video config data: {} bytes",
                    config_data.len()
                );
                // Set the extradata on the codec context — this contains
                // SPS/PPS for H.264 or VPS/SPS/PPS for H.265.
                unsafe {
                    let extradata_ptr =
                        ffmpeg::ffi::av_mallocz(config_data.len() + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize)
                            as *mut u8;
                    if !extradata_ptr.is_null() {
                        std::ptr::copy_nonoverlapping(
                            config_data.as_ptr(),
                            extradata_ptr,
                            config_data.len(),
                        );
                        (*context.as_mut_ptr()).extradata = extradata_ptr;
                        (*context.as_mut_ptr()).extradata_size = config_data.len() as i32;
                    }
                }
            }
            Err(_) => {
                warn!("No video config received within 30s, attempting to decode without it");
            }
        }
    }

    let mut decoder = context.decoder().video().context("Failed to open video decoder")?;

    let mut stats = DecoderStats::default();
    let mut scaler: Option<scaling::Context> = None;

    // Target pixel format for v4l2loopback
    let target_format = ffmpeg::format::Pixel::YUYV422;
    let target_width = width;
    let target_height = height;

    info!("Video decoder ready, processing frames...");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Also drain any new config data (codec reconfiguration)
        while let Ok(config) = video_config_rx.try_recv() {
            debug!("Received updated video config: {} bytes", config.len());
            // For mid-stream config changes, we feed it as a packet
            let mut pkt = ffmpeg::Packet::copy(&config);
            pkt.set_pts(None);
            pkt.set_dts(None);
            if let Err(e) = decoder.send_packet(&pkt) {
                warn!("Failed to send config packet to decoder: {}", e);
            }
        }

        // Receive encoded frame data with a short timeout so we can check shutdown
        let encoded_data = match video_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(data) => data,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("Video input channel disconnected");
                break;
            }
        };

        // Create an FFmpeg packet from the raw encoded data
        let packet = ffmpeg::Packet::copy(&encoded_data);

        // Send the packet to the decoder
        if let Err(e) = decoder.send_packet(&packet) {
            debug!("Decoder send_packet error (may be transient): {}", e);
            stats.decode_errors += 1;
            continue;
        }

        // Drain all decoded frames from this packet
        let mut decoded_frame = ffmpeg::frame::Video::empty();
        while decoder.receive_frame(&mut decoded_frame).is_ok() {
            stats.frames_decoded += 1;

            let src_width = decoded_frame.width();
            let src_height = decoded_frame.height();
            let src_format = decoded_frame.format();

            // Initialize or reinitialize the scaler if the source format changed
            if scaler.is_none() {
                info!(
                    "First decoded frame: {}x{} {:?} → {}x{} {:?}",
                    src_width, src_height, src_format,
                    target_width, target_height, target_format
                );
            }

            // Create scaler on first frame or if dimensions change
            let scaler_ctx = match scaler.as_mut() {
                Some(s) => s,
                None => {
                    let s = scaling::Context::get(
                        src_format,
                        src_width,
                        src_height,
                        target_format,
                        target_width,
                        target_height,
                        scaling::Flags::BILINEAR,
                    )
                    .context("Failed to create pixel format scaler")?;
                    scaler = Some(s);
                    scaler.as_mut().expect("just inserted")
                }
            };

            // Scale/convert the decoded frame to YUYV422
            let mut output_frame = ffmpeg::frame::Video::empty();
            scaler_ctx
                .run(&decoded_frame, &mut output_frame)
                .context("Failed to scale frame")?;

            // Extract the raw YUYV422 data from the output frame.
            // YUYV422 is a packed format: 2 bytes per pixel, stored in plane 0.
            let data = output_frame.data(0);
            let stride = output_frame.stride(0);
            let frame_height = target_height as usize;

            // Copy row-by-row to handle stride != width*2
            let row_bytes = (target_width * 2) as usize;
            let mut yuyv_data = Vec::with_capacity(row_bytes * frame_height);
            for row in 0..frame_height {
                let offset = row * stride;
                let end = offset + row_bytes;
                if end <= data.len() {
                    yuyv_data.extend_from_slice(&data[offset..end]);
                }
            }

            // Send to the v4l2 output thread
            match frame_tx.try_send(yuyv_data) {
                Ok(()) => {}
                Err(crossbeam_channel::TrySendError::Full(_)) => {
                    stats.frames_dropped += 1;
                    if stats.frames_dropped % 100 == 1 {
                        debug!(
                            "Dropped {} frames (v4l2 output can't keep up)",
                            stats.frames_dropped
                        );
                    }
                }
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                    info!("Frame output channel disconnected");
                    return Ok(());
                }
            }

            if stats.frames_decoded % 300 == 0 {
                info!(
                    "Decoder stats: {} decoded, {} dropped, {} errors",
                    stats.frames_decoded, stats.frames_dropped, stats.decode_errors
                );
            }
        }
    }

    // Flush the decoder
    let _ = decoder.send_eof();
    let mut decoded_frame = ffmpeg::frame::Video::empty();
    while decoder.receive_frame(&mut decoded_frame).is_ok() {
        stats.frames_decoded += 1;
    }

    info!(
        "Decoder final stats: {} decoded, {} dropped, {} errors",
        stats.frames_decoded, stats.frames_dropped, stats.decode_errors
    );

    Ok(())
}
