// v4l2loopback virtual camera output.
//
// Opens a v4l2loopback device (e.g. /dev/video10), sets the output format
// to YUYV422, and writes decoded frames to make them appear as a real webcam.

use anyhow::{bail, Context, Result};
use crossbeam_channel::Receiver;
use log::{debug, error, info, warn};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// V4L2 constants and ioctl structures
// ---------------------------------------------------------------------------

/// V4L2_BUF_TYPE_VIDEO_OUTPUT = 2
const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;

/// V4L2_PIX_FMT_YUYV (fourcc 'YUYV')
const V4L2_PIX_FMT_YUYV: u32 = u32::from_le_bytes([b'Y', b'U', b'Y', b'V']);

/// V4L2_FIELD_NONE = 1
const V4L2_FIELD_NONE: u32 = 1;

/// V4L2_COLORSPACE_SRGB = 8
const V4L2_COLORSPACE_SRGB: u32 = 8;

/// The kernel's v4l2_pix_format struct (packed, no padding).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct V4l2PixFormat {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    priv_: u32,
    flags: u32,
    encoding_or_ycbcr: u32,
    quantization: u32,
    xfer_func: u32,
}

/// The kernel's v4l2_format struct for single-planar formats.
/// We use a raw byte union buffer to match the kernel ABI exactly.
#[repr(C)]
struct V4l2Format {
    type_: u32,
    fmt: V4l2PixFormat,
    // Padding to match the kernel's union size (200 bytes total for fmt).
    _padding: [u8; 200 - std::mem::size_of::<V4l2PixFormat>()],
}

// VIDIOC_S_FMT = _IOWR('V', 5, struct v4l2_format)
nix::ioctl_readwrite!(vidioc_s_fmt, b'V', 5, V4l2Format);

/// Open a v4l2loopback device and configure it for YUYV output.
///
/// Returns a file handle ready for writing raw YUYV422 frame data.
fn open_v4l2_device(
    device_path: &str,
    width: u32,
    height: u32,
) -> Result<std::fs::File> {
    info!(
        "Opening v4l2loopback device: {} ({}x{} YUYV)",
        device_path, width, height
    );

    let file = OpenOptions::new()
        .write(true)
        .open(device_path)
        .with_context(|| {
            format!(
                "Failed to open {}. Make sure:\n\
                 1. v4l2loopback is loaded: sudo modprobe v4l2loopback exclusive_caps=1 video_nr=10 card_label=\"CamDroid\"\n\
                 2. You have write permission: sudo usermod -aG video $USER",
                device_path
            )
        })?;

    let fd = file.as_raw_fd();

    let bytesperline = width * 2; // YUYV = 2 bytes per pixel
    let sizeimage = bytesperline * height;

    let mut fmt = V4l2Format {
        type_: V4L2_BUF_TYPE_VIDEO_OUTPUT,
        fmt: V4l2PixFormat {
            width,
            height,
            pixelformat: V4L2_PIX_FMT_YUYV,
            field: V4L2_FIELD_NONE,
            bytesperline,
            sizeimage,
            colorspace: V4L2_COLORSPACE_SRGB,
            priv_: 0,
            flags: 0,
            encoding_or_ycbcr: 0,
            quantization: 0,
            xfer_func: 0,
        },
        _padding: [0u8; 200 - std::mem::size_of::<V4l2PixFormat>()],
    };

    unsafe {
        vidioc_s_fmt(fd, &mut fmt).with_context(|| {
            format!(
                "VIDIOC_S_FMT failed on {}. The device may not support {}x{} YUYV output.",
                device_path, width, height
            )
        })?;
    }

    info!(
        "v4l2loopback configured: {}x{} YUYV (sizeimage={})",
        fmt.fmt.width, fmt.fmt.height, fmt.fmt.sizeimage
    );

    Ok(file)
}

/// Spawn a thread that reads decoded YUYV422 frames from a channel
/// and writes them to a v4l2loopback device at the target frame rate.
///
/// # Arguments
/// * `device_path` - Path to the v4l2loopback device (e.g. `/dev/video10`)
/// * `frame_rx` - Channel receiving raw YUYV422 frame bytes
/// * `width` - Frame width in pixels
/// * `height` - Frame height in pixels
/// * `target_fps` - Target output frame rate for pacing
/// * `shutdown` - Shutdown signal
pub fn spawn_v4l2_writer_thread(
    device_path: String,
    frame_rx: Receiver<Vec<u8>>,
    width: u32,
    height: u32,
    target_fps: u32,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::Builder::new()
        .name("v4l2-writer".into())
        .spawn(move || {
            if let Err(e) =
                v4l2_writer_loop(&device_path, frame_rx, width, height, target_fps, shutdown)
            {
                error!("v4l2 writer error: {:#}", e);
            }
            info!("v4l2 writer thread exiting");
        })
        .context("Failed to spawn v4l2 writer thread")?;

    Ok(handle)
}

/// Main v4l2 writer loop — opens device, writes frames at target FPS.
fn v4l2_writer_loop(
    device_path: &str,
    frame_rx: Receiver<Vec<u8>>,
    width: u32,
    height: u32,
    target_fps: u32,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let mut device = open_v4l2_device(device_path, width, height)?;

    let frame_interval = Duration::from_secs_f64(1.0 / target_fps as f64);
    let expected_frame_size = (width * height * 2) as usize; // YUYV = 2 bytes/pixel
    let mut frames_written: u64 = 0;
    let mut last_stats_time = Instant::now();
    let start_time = Instant::now();

    info!(
        "v4l2 writer ready: {} @ {}fps (frame interval: {:?})",
        device_path, target_fps, frame_interval
    );

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let frame_start = Instant::now();

        // Receive the next decoded frame
        let frame_data = match frame_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(data) => data,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("Frame channel disconnected");
                break;
            }
        };

        // Validate frame size
        if frame_data.len() != expected_frame_size {
            warn!(
                "Frame size mismatch: got {} bytes, expected {} ({}x{} YUYV)",
                frame_data.len(),
                expected_frame_size,
                width,
                height
            );
            // Try to write anyway — v4l2loopback may accept partial frames
            if frame_data.len() < expected_frame_size {
                // Pad with zeros (black)
                let mut padded = frame_data;
                padded.resize(expected_frame_size, 0x80); // 0x80 = neutral UV
                if let Err(e) = device.write_all(&padded) {
                    error!("v4l2 write error (padded): {}", e);
                    // Try reopening the device
                    device = open_v4l2_device(device_path, width, height)?;
                    continue;
                }
            } else {
                // Truncate to expected size
                if let Err(e) = device.write_all(&frame_data[..expected_frame_size]) {
                    error!("v4l2 write error (truncated): {}", e);
                    device = open_v4l2_device(device_path, width, height)?;
                    continue;
                }
            }
        } else if let Err(e) = device.write_all(&frame_data) {
            error!("v4l2 write error: {}", e);
            // Attempt to recover by reopening the device
            match open_v4l2_device(device_path, width, height) {
                Ok(new_device) => {
                    device = new_device;
                    warn!("v4l2 device reopened successfully");
                }
                Err(reopen_err) => {
                    bail!("Failed to reopen v4l2 device: {:#}", reopen_err);
                }
            }
            continue;
        }

        frames_written += 1;

        // Frame pacing — sleep for the remaining frame interval
        let elapsed = frame_start.elapsed();
        if elapsed < frame_interval {
            thread::sleep(frame_interval - elapsed);
        }

        // Log periodic statistics
        if last_stats_time.elapsed() >= Duration::from_secs(10) {
            let total_elapsed = start_time.elapsed().as_secs_f64();
            let actual_fps = frames_written as f64 / total_elapsed;
            info!(
                "v4l2 output: {} frames written, {:.1} avg fps",
                frames_written, actual_fps
            );
            last_stats_time = Instant::now();
        }
    }

    info!("v4l2 writer: {} total frames written", frames_written);
    Ok(())
}
