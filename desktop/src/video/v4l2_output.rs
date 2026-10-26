//! v4l2loopback virtual camera sink.
//!
//! Opens a v4l2loopback device (e.g. /dev/video10), sets its output format to
//! YUYV422, and writes frames so they appear as a real webcam to other apps.
//! Pacing, retries and filtering belong to the render stage; this type only
//! owns the device handle.

use anyhow::{Context, Result};
use log::info;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;

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
    _padding_align: u32, // Align the union to 8 bytes on 64-bit platforms
    fmt: V4l2PixFormat,
    // Padding to match the kernel's union size (200 bytes total for fmt).
    _padding: [u8; 200 - std::mem::size_of::<V4l2PixFormat>()],
}

// VIDIOC_S_FMT = _IOWR('V', 5, struct v4l2_format)
nix::ioctl_readwrite!(vidioc_s_fmt, b'V', 5, V4l2Format);

/// Open a v4l2loopback device and configure it for YUYV output.
///
/// Returns a file handle ready for writing raw YUYV422 frame data.
fn open_v4l2_device(device_path: &str, width: u32, height: u32) -> Result<File> {
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

    info!(
        "v4l2loopback configured: {}x{} YUYV (sizeimage={})",
        fmt.fmt.width, fmt.fmt.height, fmt.fmt.sizeimage
    );

    Ok(file)
}

/// An open, configured v4l2loopback output device.
#[derive(Debug)]
pub struct V4l2Sink {
    file: File,
    frame_len: usize,
}

impl V4l2Sink {
    /// Opens `device_path` and configures it for `width` x `height` YUYV.
    pub fn open(device_path: &str, width: u32, height: u32) -> Result<Self> {
        Ok(Self {
            file: open_v4l2_device(device_path, width, height)?,
            frame_len: (width as usize) * (height as usize) * 2,
        })
    }

    /// Size in bytes of one frame the device expects.
    pub fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Writes one complete frame. `frame` must be exactly [`Self::frame_len`] bytes.
    pub fn write_frame(&mut self, frame: &[u8]) -> std::io::Result<()> {
        debug_assert_eq!(frame.len(), self.frame_len);
        self.file.write_all(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v4l2_format_layout() {
        assert_eq!(std::mem::size_of::<V4l2PixFormat>(), 48);
        assert_eq!(std::mem::size_of::<V4l2Format>(), 208);

        let dummy = V4l2Format {
            type_: 0,
            _padding_align: 0,
            fmt: V4l2PixFormat {
                width: 0,
                height: 0,
                pixelformat: 0,
                field: 0,
                bytesperline: 0,
                sizeimage: 0,
                colorspace: 0,
                priv_: 0,
                flags: 0,
                encoding_or_ycbcr: 0,
                quantization: 0,
                xfer_func: 0,
            },
            _padding: [0u8; 200 - std::mem::size_of::<V4l2PixFormat>()],
        };

        let base_ptr = &dummy as *const V4l2Format as usize;
        let fmt_ptr = &dummy.fmt as *const V4l2PixFormat as usize;
        let width_ptr = &dummy.fmt.width as *const u32 as usize;
        let pixelformat_ptr = &dummy.fmt.pixelformat as *const u32 as usize;

        assert_eq!(fmt_ptr - base_ptr, 8);
        assert_eq!(width_ptr - base_ptr, 8);
        assert_eq!(pixelformat_ptr - base_ptr, 16);
    }

    #[test]
    fn opening_a_missing_device_explains_how_to_fix_it() {
        let error = V4l2Sink::open("/dev/camdroid-does-not-exist", 1920, 1080).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("modprobe v4l2loopback"), "got: {message}");
    }
}
