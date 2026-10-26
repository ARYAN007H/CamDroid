//! FFmpeg-based video decoder for H.264, H.265/HEVC, and MJPEG streams.
//!
//! Receives encoded access units, decodes them, and converts the newest
//! picture to packed YUYV422 for the render stage.
//!
//! Every admitted packet is decoded in order, even when the decoder is
//! briefly behind: inter-coded video cannot skip a frame without corrupting
//! the ones after it. Latency is controlled instead by converting and
//! forwarding only the newest picture of each batch, which is where most of
//! the per-frame cost lies.

use crate::connection::KeyframeRequester;
use crate::media_queue::TimedMediaFrame;
use crate::protocol::v2::MediaKind;
use crate::telemetry::Telemetry;
use anyhow::{bail, Context, Result};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

extern crate ffmpeg_next as ffmpeg;

use ffmpeg::codec;
use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;

/// Capture timestamps travel as nanoseconds.
const MEDIA_TIME_BASE: ffmpeg::Rational = ffmpeg::Rational(1, 1_000_000_000);
/// How long to wait for the codec configuration before decoding without it.
const CONFIG_TIMEOUT: Duration = Duration::from_secs(30);
/// Consecutive rejected packets before a fresh key frame is requested.
const KEYFRAME_REQUEST_AFTER_ERRORS: u32 = 3;

/// Supported video codecs that the decoder can handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Mjpeg,
}

impl std::str::FromStr for VideoCodec {
    type Err = anyhow::Error;

    /// Parses a codec name such as `"h264"`, `"HEVC"` or `"mjpeg"`.
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "h264" | "avc" => Ok(VideoCodec::H264),
            "h265" | "hevc" => Ok(VideoCodec::H265),
            "mjpeg" | "jpeg" => Ok(VideoCodec::Mjpeg),
            _ => bail!("Unsupported codec: {}", s),
        }
    }
}

impl VideoCodec {
    /// Get the FFmpeg codec ID for this video codec.
    fn ffmpeg_id(&self) -> codec::Id {
        match self {
            VideoCodec::H264 => codec::Id::H264,
            VideoCodec::H265 => codec::Id::HEVC,
            VideoCodec::Mjpeg => codec::Id::MJPEG,
        }
    }
}

/// What to decode and the geometry to produce.
#[derive(Debug, Clone, Copy)]
pub struct DecoderConfig {
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
}

/// Channels feeding and draining the decoder.
pub struct DecoderChannels {
    pub video_config_rx: Receiver<Vec<u8>>,
    pub video_rx: Receiver<TimedMediaFrame>,
    pub frame_tx: Sender<Vec<u8>>,
}

/// Spawns the decoder thread. Decoded YUYV422 frames are sent on
/// `channels.frame_tx`; health is reported through `telemetry`.
pub fn spawn_decoder_thread(
    config: DecoderConfig,
    channels: DecoderChannels,
    telemetry: Telemetry,
    keyframes: KeyframeRequester,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("video-decoder".into())
        .spawn(move || {
            if let Err(e) = decoder_loop(config, channels, &telemetry, &keyframes, &shutdown) {
                error!("Video decoder error: {:#}", e);
                telemetry.set_disconnect_reason(format!("Video decoder stopped: {e:#}"));
            }
            info!("Video decoder thread exiting");
        })
        .context("Failed to spawn video decoder thread")
}

/// Hands codec configuration (SPS/PPS or VPS/SPS/PPS) to FFmpeg.
fn set_extradata(context: &mut codec::Context, config: &[u8]) {
    // SAFETY: the buffer is allocated with av_mallocz plus the padding FFmpeg
    // requires, fully initialised, and ownership transfers to the codec
    // context, which frees it with the context.
    unsafe {
        let padded = config.len() + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
        let buffer = ffmpeg::ffi::av_mallocz(padded) as *mut u8;
        if buffer.is_null() {
            warn!("Out of memory while storing codec configuration");
            return;
        }
        std::ptr::copy_nonoverlapping(config.as_ptr(), buffer, config.len());
        (*context.as_mut_ptr()).extradata = buffer;
        (*context.as_mut_ptr()).extradata_size = config.len() as i32;
    }
}

fn build_packet(unit: &TimedMediaFrame) -> ffmpeg::Packet {
    let mut packet = ffmpeg::Packet::copy(&unit.packet.payload);
    if unit.packet.session_id.0 != 0 {
        let pts = i64::try_from(unit.packet.capture_timestamp.0).unwrap_or(i64::MAX);
        packet.set_pts(Some(pts));
        packet.set_dts(Some(pts));
        packet.set_time_base(MEDIA_TIME_BASE);
    }
    packet
}

/// A swscale context together with the source format it was built for.
struct Converter {
    context: scaling::Context,
    source: (Pixel, u32, u32),
}

impl Converter {
    /// Converts `decoded` to YUYV422 at the target size, reusing the existing
    /// context unless the decoder's output format changed mid-stream.
    fn to_yuyv(
        slot: &mut Option<Self>,
        decoded: &ffmpeg::frame::Video,
        target_width: u32,
        target_height: u32,
    ) -> Result<Vec<u8>> {
        let source = (decoded.format(), decoded.width(), decoded.height());
        if slot.as_ref().map(|c| c.source) != Some(source) {
            info!(
                "Decoded picture {}x{} {:?} -> {}x{} YUYV422",
                source.1, source.2, source.0, target_width, target_height
            );
            *slot = Some(Self {
                context: scaling::Context::get(
                    source.0,
                    source.1,
                    source.2,
                    Pixel::YUYV422,
                    target_width,
                    target_height,
                    scaling::Flags::BILINEAR,
                )
                .context("Failed to create pixel format scaler")?,
                source,
            });
        }
        let converter = slot.as_mut().expect("converter initialised above");

        let mut output = ffmpeg::frame::Video::empty();
        converter
            .context
            .run(decoded, &mut output)
            .context("Failed to convert decoded frame")?;

        // YUYV422 is packed, 2 bytes per pixel, in plane 0. Copy row by row
        // because FFmpeg pads each row to its alignment.
        let data = output.data(0);
        let stride = output.stride(0);
        let row_bytes = target_width as usize * 2;
        let mut yuyv = Vec::with_capacity(row_bytes * target_height as usize);
        for row in 0..target_height as usize {
            let start = row * stride;
            let end = start + row_bytes;
            if end > data.len() {
                bail!("converted frame is smaller than {target_width}x{target_height}");
            }
            yuyv.extend_from_slice(&data[start..end]);
        }
        Ok(yuyv)
    }
}

fn decoder_loop(
    config: DecoderConfig,
    channels: DecoderChannels,
    telemetry: &Telemetry,
    keyframes: &KeyframeRequester,
    shutdown: &AtomicBool,
) -> Result<()> {
    let DecoderChannels {
        video_config_rx,
        video_rx,
        frame_tx,
    } = channels;
    info!(
        "Video decoder starting: {:?} {}x{}",
        config.codec, config.width, config.height
    );

    let codec = codec::decoder::find(config.codec.ffmpeg_id())
        .with_context(|| format!("FFmpeg decoder not found for {:?}", config.codec))?;
    let mut context = codec::Context::new_with_codec(codec);
    context.set_time_base(MEDIA_TIME_BASE);

    // MJPEG frames are self-contained. H.264/H.265 need their parameter sets
    // before the first picture can be decoded.
    if config.codec != VideoCodec::Mjpeg {
        info!("Waiting for video codec configuration...");
        match video_config_rx.recv_timeout(CONFIG_TIMEOUT) {
            Ok(parameters) => {
                info!("Received video config data: {} bytes", parameters.len());
                set_extradata(&mut context, &parameters);
            }
            Err(_) => warn!(
                "No video config received within {}s, decoding without it",
                CONFIG_TIMEOUT.as_secs()
            ),
        }
    }

    let mut decoder = context
        .decoder()
        .video()
        .context("Failed to open video decoder")?;
    let mut converter: Option<Converter> = None;
    let mut consecutive_errors = 0_u32;
    info!("Video decoder ready, processing frames...");

    while !shutdown.load(Ordering::Relaxed) {
        // Parameter sets re-sent mid-stream (for example after a camera
        // switch) are fed to the decoder as ordinary packets.
        while let Ok(parameters) = video_config_rx.try_recv() {
            debug!("Received updated video config: {} bytes", parameters.len());
            let mut packet = ffmpeg::Packet::copy(&parameters);
            packet.set_pts(None);
            packet.set_dts(None);
            if let Err(e) = decoder.send_packet(&packet) {
                warn!("Failed to send config packet to decoder: {e}");
            }
        }

        let first = match video_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(unit) => unit,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("Video input channel disconnected");
                break;
            }
        };

        let mut newest: Option<ffmpeg::frame::Video> = None;
        let mut decoded_in_batch = 0_u64;
        let batch = std::iter::once(first).chain(std::iter::from_fn(|| video_rx.try_recv().ok()));
        for unit in batch.filter(|unit| unit.packet.kind == MediaKind::Video) {
            match decoder.send_packet(&build_packet(&unit)) {
                Ok(()) => consecutive_errors = 0,
                Err(e) => {
                    debug!("Decoder rejected a packet: {e}");
                    telemetry.record_decode_error();
                    consecutive_errors += 1;
                    if consecutive_errors >= KEYFRAME_REQUEST_AFTER_ERRORS {
                        keyframes.request();
                        consecutive_errors = 0;
                    }
                    continue;
                }
            }
            loop {
                let mut picture = ffmpeg::frame::Video::empty();
                if decoder.receive_frame(&mut picture).is_err() {
                    break;
                }
                decoded_in_batch += 1;
                newest = Some(picture);
            }
        }

        telemetry.record_decoded(decoded_in_batch);
        telemetry.record_dropped(decoded_in_batch.saturating_sub(1));

        let Some(picture) = newest else {
            continue;
        };
        match Converter::to_yuyv(&mut converter, &picture, config.width, config.height) {
            Ok(yuyv) => match frame_tx.try_send(yuyv) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => telemetry.record_dropped(1),
                Err(TrySendError::Disconnected(_)) => {
                    info!("Frame output channel disconnected");
                    return Ok(());
                }
            },
            Err(e) => {
                warn!("Dropping a picture that could not be converted: {e:#}");
                telemetry.record_decode_error();
            }
        }
    }

    let _ = decoder.send_eof();
    info!("Video decoder stopped");
    Ok(())
}
