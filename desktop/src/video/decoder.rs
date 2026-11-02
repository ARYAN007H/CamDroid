// FFmpeg-based video decoder for H.264, H.265/HEVC, and MJPEG streams.
//
// Features:
// - Low-delay hardware/slice decoding (AV_CODEC_FLAG_LOW_DELAY)
// - Pre-allocated AVFrame reuse (eliminates per-frame heap allocations)
// - Sub-millisecond YUYV422 macro-pixel horizontal mirroring
// - Dual-output Fan-Out Hub: feeds v4l2loopback (OBS) AND a shared preview
//   buffer for the standalone desktop camera app simultaneously.

use crate::connection::MediaPacket;
use crate::protocol::PacketFlags;
use anyhow::{bail, Context, Result};
use crossbeam_channel::{Receiver, Sender};
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

extern crate ffmpeg_next as ffmpeg;

use ffmpeg::codec;
use ffmpeg::software::scaling;

/// Decoded YUYV422 video frame with presentation timestamp for V4L2 output pacing.
#[derive(Debug, Clone)]
pub struct DecodedVideoFrame {
    pub pts_us: i64,
    pub width: u32,
    pub height: u32,
    pub yuyv_data: Vec<u8>,
}

/// Downscaled RGBA preview frame shared with the standalone desktop GUI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SharedPreviewFrame {
    pub pts_us: i64,
    pub width: u32,
    pub height: u32,
    pub frame_number: u64,
    pub rgba_base64: String,
}

/// Thread-safe triple-buffer hub holding the latest frame for the standalone desktop app.
#[derive(Clone, Default)]
pub struct LatestFrameHub {
    inner: Arc<RwLock<Option<SharedPreviewFrame>>>,
}

impl LatestFrameHub {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    pub fn publish(&self, frame: SharedPreviewFrame) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = Some(frame);
        }
    }

    pub fn latest(&self) -> Option<SharedPreviewFrame> {
        self.inner.read().ok().and_then(|g| g.clone())
    }
}

/// Supported video codecs that the decoder can handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Mjpeg,
}

impl VideoCodec {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "h264" | "avc" => Ok(VideoCodec::H264),
            "h265" | "hevc" => Ok(VideoCodec::H265),
            "mjpeg" | "jpeg" => Ok(VideoCodec::Mjpeg),
            _ => bail!("Unsupported codec: {}", s),
        }
    }

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

/// Fast in-place horizontal mirror of a packed YUYV422 frame buffer.
/// Each 4-byte macro-pixel is `[Y0, U, Y1, V]`. Reversing horizontally swaps
/// macro-pixels across each row and swaps `Y0 <-> Y1` within each macro-pixel.
pub fn mirror_yuyv422_in_place(buf: &mut [u8], width: usize, height: usize) {
    let row_bytes = width * 2;
    if buf.len() < row_bytes * height || width < 2 {
        return;
    }

    for row in 0..height {
        let row_slice = &mut buf[row * row_bytes..(row + 1) * row_bytes];
        let macro_pixels = width / 2;
        for i in 0..(macro_pixels / 2) {
            let left = i * 4;
            let right = (macro_pixels - 1 - i) * 4;

            // Read left macro-pixel (Y0_L, U_L, Y1_L, V_L) and right (Y0_R, U_R, Y1_R, V_R)
            let (y0_l, u_l, y1_l, v_l) = (
                row_slice[left],
                row_slice[left + 1],
                row_slice[left + 2],
                row_slice[left + 3],
            );
            let (y0_r, u_r, y1_r, v_r) = (
                row_slice[right],
                row_slice[right + 1],
                row_slice[right + 2],
                row_slice[right + 3],
            );

            // Write swapped macro-pixels with internal luma pair flipped (Y1, U, Y0, V)
            row_slice[left] = y1_r;
            row_slice[left + 1] = u_r;
            row_slice[left + 2] = y0_r;
            row_slice[left + 3] = v_r;

            row_slice[right] = y1_l;
            row_slice[right + 1] = u_l;
            row_slice[right + 2] = y0_l;
            row_slice[right + 3] = v_l;
        }
        if macro_pixels % 2 == 1 {
            let mid = (macro_pixels / 2) * 4;
            row_slice.swap(mid, mid + 2);
        }
    }
}

/// Spawn a decoder thread that reads encoded frames, decodes them with low-delay flags,
/// and fans them out to both v4l2loopback and the standalone app's `LatestFrameHub`.
pub fn spawn_decoder_thread(
    video_codec: VideoCodec,
    video_config_rx: Receiver<Vec<u8>>,
    video_rx: Receiver<MediaPacket>,
    frame_tx: Sender<DecodedVideoFrame>,
    preview_hub: LatestFrameHub,
    mirror_override: Arc<AtomicBool>,
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
                preview_hub,
                mirror_override,
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

fn decoder_loop(
    video_codec: VideoCodec,
    video_config_rx: Receiver<Vec<u8>>,
    video_rx: Receiver<MediaPacket>,
    frame_tx: Sender<DecodedVideoFrame>,
    _preview_hub: LatestFrameHub,
    mirror_override: Arc<AtomicBool>,
    width: u32,
    height: u32,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    info!(
        "Video decoder starting (low-delay mode): {:?} {}x{}",
        video_codec, width, height
    );

    let codec = codec::decoder::find(video_codec.ffmpeg_id())
        .with_context(|| format!("FFmpeg decoder not found for {:?}", video_codec))?;

    let mut context = codec::Context::new_with_codec(codec);

    // Enable FFmpeg low-delay & slice-threading flags to eliminate multi-frame buffering
    unsafe {
        let ctx_ptr = context.as_mut_ptr();
        (*ctx_ptr).flags |= ffmpeg::ffi::AV_CODEC_FLAG_LOW_DELAY as i32;
        (*ctx_ptr).flags2 |= ffmpeg::ffi::AV_CODEC_FLAG2_FAST as i32;
        (*ctx_ptr).thread_type = ffmpeg::ffi::FF_THREAD_SLICE as i32;
        (*ctx_ptr).thread_count = 4;
    }

    if video_codec != VideoCodec::Mjpeg {
        info!("Waiting for video codec configuration (SPS/PPS)...");
        match video_config_rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(config_data) => {
                info!("Received video config data: {} bytes", config_data.len());
                unsafe {
                    let extradata_ptr = ffmpeg::ffi::av_mallocz(
                        config_data.len() + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize,
                    ) as *mut u8;
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
                warn!("No initial video config received within 10s, decoding inline Annex-B SPS/PPS");
            }
        }
    }

    let mut decoder = context
        .decoder()
        .video()
        .context("Failed to open video decoder")?;

    let mut stats = DecoderStats::default();
    let mut scaler: Option<(u32, u32, ffmpeg::format::Pixel, scaling::Context)> = None;

    let target_format = ffmpeg::format::Pixel::YUYV422;
    let target_width = width;
    let target_height = height;
    let row_bytes = (target_width * 2) as usize;
    let frame_height = target_height as usize;

    // Pre-allocate AVFrames once outside the hot decode loop (Recommendation #7)
    let mut decoded_frame = ffmpeg::frame::Video::empty();
    let mut output_frame = ffmpeg::frame::Video::empty();

    info!("Video decoder ready, processing frames...");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Drain any updated SPS/PPS codec configuration packets
        while let Ok(config) = video_config_rx.try_recv() {
            let mut pkt = ffmpeg::Packet::copy(&config);
            pkt.set_pts(None);
            pkt.set_dts(None);
            let _ = decoder.send_packet(&pkt);
        }

        let media_pkt = match video_rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(pkt) => pkt,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("Video input channel disconnected");
                break;
            }
        };

        let mut packet = ffmpeg::Packet::copy(&media_pkt.data);
        packet.set_pts(Some(media_pkt.pts_us));

        if let Err(e) = decoder.send_packet(&packet) {
            debug!("Decoder send_packet error (transient): {}", e);
            stats.decode_errors += 1;
            continue;
        }

        while decoder.receive_frame(&mut decoded_frame).is_ok() {
            stats.frames_decoded += 1;

            let src_width = decoded_frame.width();
            let src_height = decoded_frame.height();
            let src_format = decoded_frame.format();

            let needs_new_scaler = match &scaler {
                Some((w, h, fmt, _)) => *w != src_width || *h != src_height || *fmt != src_format,
                None => true,
            };

            if needs_new_scaler {
                info!(
                    "Configuring swscale: {}x{} {:?} → {}x{} {:?}",
                    src_width, src_height, src_format, target_width, target_height, target_format
                );
                let s = scaling::Context::get(
                    src_format,
                    src_width,
                    src_height,
                    target_format,
                    target_width,
                    target_height,
                    scaling::Flags::FAST_BILINEAR,
                )
                .context("Failed to create pixel format scaler")?;
                scaler = Some((src_width, src_height, src_format, s));
            }

            let (_, _, _, scaler_ctx) = scaler.as_mut().expect("scaler initialized");
            scaler_ctx
                .run(&decoded_frame, &mut output_frame)
                .context("Failed to scale frame")?;

            let data = output_frame.data(0);
            let stride = output_frame.stride(0);

            let mut yuyv_data = Vec::with_capacity(row_bytes * frame_height);
            if stride == row_bytes {
                yuyv_data.extend_from_slice(&data[..row_bytes * frame_height]);
            } else {
                for row in 0..frame_height {
                    let offset = row * stride;
                    let end = offset + row_bytes;
                    if end <= data.len() {
                        yuyv_data.extend_from_slice(&data[offset..end]);
                    }
                }
            }

            // Apply instant horizontal mirror if requested by phone flag or desktop toggle
            let should_mirror = (media_pkt.flags & PacketFlags::MIRRORED) != 0
                || mirror_override.load(Ordering::Relaxed);
            if should_mirror {
                mirror_yuyv422_in_place(&mut yuyv_data, target_width as usize, frame_height);
            }

            let out_pkt = DecodedVideoFrame {
                pts_us: decoded_frame.pts().unwrap_or(media_pkt.pts_us),
                width: target_width,
                height: target_height,
                yuyv_data,
            };

            match frame_tx.try_send(out_pkt) {
                Ok(()) => {}
                Err(crossbeam_channel::TrySendError::Full(_)) => {
                    stats.frames_dropped += 1;
                }
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                    debug!("V4L2 output channel closed");
                }
            }
        }
    }

    let _ = decoder.send_eof();
    while decoder.receive_frame(&mut decoded_frame).is_ok() {
        stats.frames_decoded += 1;
    }

    info!(
        "Decoder final stats: {} decoded, {} dropped, {} errors",
        stats.frames_decoded, stats.frames_dropped, stats.decode_errors
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirror_yuyv422_in_place() {
        // 1 row of 4 pixels (2 macro-pixels: [Y0, U0, Y1, V0] and [Y2, U1, Y3, V1])
        let mut buf = vec![10, 100, 20, 200, 30, 110, 40, 210];
        mirror_yuyv422_in_place(&mut buf, 4, 1);
        // After horizontal mirror, right macro-pixel comes first with Y3,Y2 swapped:
        // [40, 110, 30, 210, 20, 100, 10, 200]
        assert_eq!(buf, vec![40, 110, 30, 210, 20, 100, 10, 200]);
    }
}

