// Cross-platform Virtual Camera Output (v4l2loopback on Linux, standalone/OS bridge on macOS & Windows).

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use log::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use anyhow::bail;
#[cfg(target_os = "linux")]
use log::error;
#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;

#[cfg(target_os = "linux")]
const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
#[cfg(target_os = "linux")]
const V4L2_PIX_FMT_YUYV: u32 = u32::from_le_bytes([b'Y', b'U', b'Y', b'V']);
#[cfg(target_os = "linux")]
const V4L2_FIELD_NONE: u32 = 1;
#[cfg(target_os = "linux")]
const V4L2_COLORSPACE_SRGB: u32 = 8;

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
#[repr(C)]
struct V4l2Format {
    type_: u32,
    _padding_align: u32,
    fmt: V4l2PixFormat,
    _padding: [u8; 200 - std::mem::size_of::<V4l2PixFormat>()],
}

#[cfg(target_os = "linux")]
nix::ioctl_readwrite!(vidioc_s_fmt, b'V', 5, V4l2Format);

#[cfg(target_os = "linux")]
fn open_v4l2_device(device_path: &str, width: u32, height: u32) -> Result<std::fs::File> {
    info!(
        "Opening v4l2loopback device: {} ({}x{} YUYV)",
        device_path, width, height
    );

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(device_path)
        .with_context(|| {
            format!(
                "Failed to open {}. Load v4l2loopback with: sudo modprobe v4l2loopback exclusive_caps=1 video_nr=10 card_label=\"CamDroid\"",
                device_path
            )
        })?;

    let fd = file.as_raw_fd();
    let bytesperline = width * 2;
    let sizeimage = bytesperline * height;

    let mut fmt = V4l2Format {
        type_: V4L2_BUF_TYPE_VIDEO_OUTPUT,
        _padding_align: 0,
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

    Ok(file)
}

/// Spawn a thread that reads decoded YUYV422 frames from a channel
/// and writes them to a virtual camera sink using PTS-aware pacing.
pub fn spawn_v4l2_writer_thread(
    device_path: String,
    frame_rx: Receiver<crate::video::DecodedVideoFrame>,
    width: u32,
    height: u32,
    target_fps: u32,
    shutdown: Arc<AtomicBool>,
    v4l2_active: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::Builder::new()
        .name("v4l2-writer".into())
        .spawn(move || {
            if let Err(e) = v4l2_writer_loop(
                &device_path,
                frame_rx,
                width,
                height,
                target_fps,
                shutdown,
                v4l2_active,
            ) {
                warn!("v4l2 writer exited: {:#}", e);
            }
            info!("v4l2 writer thread exiting");
        })
        .context("Failed to spawn v4l2 writer thread")?;

    Ok(handle)
}

#[cfg(target_os = "linux")]
fn v4l2_writer_loop(
    device_path: &str,
    frame_rx: Receiver<crate::video::DecodedVideoFrame>,
    width: u32,
    height: u32,
    target_fps: u32,
    shutdown: Arc<AtomicBool>,
    v4l2_active: Arc<AtomicBool>,
) -> Result<()> {
    let mut device = match open_v4l2_device(device_path, width, height) {
        Ok(dev) => {
            v4l2_active.store(true, Ordering::Relaxed);
            Some(dev)
        }
        Err(e) => {
            v4l2_active.store(false, Ordering::Relaxed);
            warn!(
                "v4l2loopback device '{}' unavailable ({}). Standalone app preview remains active.",
                device_path, e
            );
            None
        }
    };

    let max_frame_interval = Duration::from_secs_f64(1.0 / target_fps.max(1) as f64);
    let expected_frame_size = (width * height * 2) as usize;
    let mut last_pts_us: Option<i64> = None;
    let mut last_write_instant = Instant::now();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let frame = match frame_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(f) => f,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };

        if let Some(prev_pts) = last_pts_us {
            let pts_delta_us =
                (frame.pts_us - prev_pts).clamp(0, max_frame_interval.as_micros() as i64);
            let target_delta = Duration::from_micros(pts_delta_us as u64);
            let wall_elapsed = last_write_instant.elapsed();
            if wall_elapsed < target_delta && frame_rx.is_empty() {
                thread::sleep(target_delta - wall_elapsed);
            }
        }
        last_pts_us = Some(frame.pts_us);
        last_write_instant = Instant::now();

        let Some(ref mut dev) = device else {
            continue;
        };

        let frame_data = &frame.yuyv_data;
        let write_res = if frame_data.len() >= expected_frame_size {
            dev.write_all(&frame_data[..expected_frame_size])
        } else {
            let mut padded = frame_data.clone();
            padded.resize(expected_frame_size, 0x80);
            dev.write_all(&padded)
        };

        if let Err(e) = write_res {
            error!("v4l2 write error: {}", e);
            device = open_v4l2_device(device_path, width, height).ok();
            v4l2_active.store(device.is_some(), Ordering::Relaxed);
        }
    }

    v4l2_active.store(false, Ordering::Relaxed);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn v4l2_writer_loop(
    _device_path: &str,
    frame_rx: Receiver<crate::video::DecodedVideoFrame>,
    _width: u32,
    _height: u32,
    _target_fps: u32,
    shutdown: Arc<AtomicBool>,
    v4l2_active: Arc<AtomicBool>,
) -> Result<()> {
    // On macOS and Windows, frames are consumed by the standalone Continuity Camera
    // preview hub (LatestFrameHub) and OBS Window/Spout/Syphon capture. Drain channel cleanly.
    v4l2_active.store(false, Ordering::Relaxed);
    while !shutdown.load(Ordering::Relaxed) {
        match frame_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(_) => {}
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
