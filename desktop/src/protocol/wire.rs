// CamDroid Wire Protocol v2.0
//
// Binary protocol for streaming video, audio, telemetry, and control commands
// between the Android phone (server) and the desktop client.
//
// Packet format (14-byte header):
// [Type: 1B] [Flags: 1B] [PTS_us: 8B LE] [Length: 4B LE] [Payload: N bytes]

use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum allowed payload size (16 MB) to prevent memory exhaustion
/// from malformed or corrupted packets.
const MAX_PAYLOAD_SIZE: u32 = 16 * 1024 * 1024;

/// Bitmask flags carried in the 2nd byte of the v2.0 packet header.
pub mod PacketFlags {
    pub const NONE: u8 = 0x00;
    pub const KEYFRAME: u8 = 0x01;
    pub const CONFIG: u8 = 0x02;
    pub const MIRRORED: u8 = 0x04;
}

/// Identifies the type of data carried in a protocol packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    /// Encoded video frame (H.264 NAL unit, H.265 NAL unit, or JPEG)
    VideoFrame = 0x01,
    /// Encoded audio frame (AAC with ADTS header)
    AudioFrame = 0x02,
    /// JSON-encoded control command (PC → Phone)
    ControlCmd = 0x03,
    /// JSON-encoded metadata/configuration (bidirectional)
    Metadata = 0x04,
    /// Keepalive heartbeat (empty payload)
    Heartbeat = 0x05,
    /// Video codec configuration (SPS/PPS for H.264, VPS/SPS/PPS for H.265)
    VideoConfig = 0x06,
    /// Audio codec configuration (AudioSpecificConfig for AAC)
    AudioConfig = 0x07,
}

impl PacketType {
    /// Parse a raw byte into a PacketType variant.
    pub fn from_byte(b: u8) -> Result<Self> {
        match b {
            0x01 => Ok(PacketType::VideoFrame),
            0x02 => Ok(PacketType::AudioFrame),
            0x03 => Ok(PacketType::ControlCmd),
            0x04 => Ok(PacketType::Metadata),
            0x05 => Ok(PacketType::Heartbeat),
            0x06 => Ok(PacketType::VideoConfig),
            0x07 => Ok(PacketType::AudioConfig),
            _ => bail!("Unknown packet type: 0x{:02X}", b),
        }
    }
}

/// A single protocol packet with its type, flags, hardware timestamp, and payload.
#[derive(Debug, Clone)]
pub struct Packet {
    pub packet_type: PacketType,
    pub flags: u8,
    pub pts_us: i64,
    pub payload: Vec<u8>,
}

fn now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

impl Packet {
    /// Create a new packet with the given type and payload data.
    pub fn new(packet_type: PacketType, payload: Vec<u8>) -> Self {
        Self {
            packet_type,
            flags: PacketFlags::NONE,
            pts_us: now_us(),
            payload,
        }
    }

    /// Create a packet with explicit flags and presentation timestamp.
    pub fn with_pts(packet_type: PacketType, flags: u8, pts_us: i64, payload: Vec<u8>) -> Self {
        Self {
            packet_type,
            flags,
            pts_us,
            payload,
        }
    }

    pub fn is_keyframe(&self) -> bool {
        (self.flags & PacketFlags::KEYFRAME) != 0
    }

    pub fn is_config(&self) -> bool {
        (self.flags & PacketFlags::CONFIG) != 0
    }

    pub fn is_mirrored(&self) -> bool {
        (self.flags & PacketFlags::MIRRORED) != 0
    }

    /// Create an empty heartbeat packet carrying current timestamp.
    pub fn heartbeat() -> Self {
        Self::new(PacketType::Heartbeat, Vec::new())
    }

    /// Create a control command packet from a JSON-serializable command.
    pub fn control_cmd(cmd: &ControlCommand) -> Result<Self> {
        let json = serde_json::to_vec(cmd).context("Failed to serialize control command")?;
        Ok(Self::new(PacketType::ControlCmd, json))
    }

    /// Create a metadata packet from a JSON-serializable value.
    pub fn metadata(meta: &serde_json::Value) -> Result<Self> {
        let json = serde_json::to_vec(meta).context("Failed to serialize metadata")?;
        Ok(Self::new(PacketType::Metadata, json))
    }
}

/// Read a packet into a caller-supplied reusable buffer, then swap/clone only what is needed.
pub fn read_packet_into(
    reader: &mut impl Read,
    scratch: &mut Vec<u8>,
) -> Result<(PacketType, u8, i64)> {
    let type_byte = reader
        .read_u8()
        .context("Failed to read packet type (connection closed?)")?;
    let packet_type = PacketType::from_byte(type_byte)?;
    let flags = reader
        .read_u8()
        .context("Failed to read packet flags")?;
    let pts_us = reader
        .read_i64::<LittleEndian>()
        .context("Failed to read packet PTS_us")?;
    let length = reader
        .read_u32::<LittleEndian>()
        .context("Failed to read packet length")?;

    if length > MAX_PAYLOAD_SIZE {
        bail!(
            "Packet payload too large: {} bytes (max {})",
            length,
            MAX_PAYLOAD_SIZE
        );
    }

    scratch.resize(length as usize, 0u8);
    if length > 0 {
        reader
            .read_exact(&mut scratch[..length as usize])
            .context("Failed to read packet payload (connection closed mid-packet?)")?;
    }

    Ok((packet_type, flags, pts_us))
}

/// Read exactly one complete v2.0 packet from the given reader.
pub fn read_packet(reader: &mut impl Read) -> Result<Packet> {
    let mut payload = Vec::new();
    let (packet_type, flags, pts_us) = read_packet_into(reader, &mut payload)?;
    Ok(Packet {
        packet_type,
        flags,
        pts_us,
        payload,
    })
}

/// Write a complete v2.0 packet (14-byte header + payload) to the given writer.
pub fn write_packet(writer: &mut impl Write, packet: &Packet) -> Result<()> {
    writer
        .write_u8(packet.packet_type as u8)
        .context("Failed to write packet type")?;
    writer
        .write_u8(packet.flags)
        .context("Failed to write packet flags")?;
    writer
        .write_i64::<LittleEndian>(packet.pts_us)
        .context("Failed to write packet PTS_us")?;
    writer
        .write_u32::<LittleEndian>(packet.payload.len() as u32)
        .context("Failed to write packet length")?;
    if !packet.payload.is_empty() {
        writer
            .write_all(&packet.payload)
            .context("Failed to write packet payload")?;
    }
    writer.flush().context("Failed to flush packet")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Control command types exchanged with the Android app
// ---------------------------------------------------------------------------

/// A control command sent from the desktop client to the phone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlCommand {
    /// Begin streaming with the specified configuration.
    Start {
        codec: String,
        resolution: String,
        fps: u32,
        audio: bool,
    },
    /// Stop the current stream gracefully.
    Stop,
    /// Switch between front and rear cameras.
    SwitchCamera,
    /// Set the camera zoom ratio (e.g. 0.5 for ultra-wide, 1.0 for main, 2.0+ for telephoto).
    SetZoom { value: f32 },
    /// Set focus mode ("auto", "manual", or "tap") and optional manual distance or tap coordinates.
    SetFocus {
        mode: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        distance: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        x: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        y: Option<f32>,
    },
    /// Set exposure compensation in EV steps (-4 to +4).
    SetExposure { compensation: i32 },
    /// Set manual ISO and shutter speed in nanoseconds.
    SetManualExposure {
        #[serde(skip_serializing_if = "Option::is_none")]
        iso: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        shutter_ns: Option<i64>,
    },
    /// Set white balance mode (auto, daylight, tungsten, fluorescent, cloudy).
    SetWhiteBalance { mode: String },
    /// Enable or disable the camera torch/flash.
    SetFlash { enabled: bool },
    /// Enable or disable horizontal video mirroring.
    SetMirror { enabled: bool },
    /// Change the streaming resolution.
    SetResolution { value: String },
    /// Change the target frame rate.
    SetFps { value: u32 },
    /// Change the video codec.
    SetCodec { value: String },
    /// Set a fixed target bitrate in bits per second. Use 0 for adaptive.
    SetBitrate { value: u64 },
    /// Request an immediate keyframe (I-frame) from the encoder.
    RequestKeyframe,
}

/// Metadata sent during the initial handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientHandshake {
    pub version: String,
    pub client: String,
}

/// Server capabilities reported during the handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub version: String,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub codecs: Vec<String>,
    #[serde(default)]
    pub resolutions: Vec<String>,
    #[serde(default)]
    pub fps: Vec<u32>,
    #[serde(default)]
    pub audio: bool,
    #[serde(default)]
    pub battery: u32,
    #[serde(default = "default_min_zoom")]
    pub min_zoom: f32,
    #[serde(default = "default_max_zoom")]
    pub max_zoom: f32,
}

fn default_min_zoom() -> f32 {
    0.5
}
fn default_max_zoom() -> f32 {
    10.0
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            version: "2.0".to_string(),
            device: "Android Camera".to_string(),
            codecs: vec!["h264".to_string(), "h265".to_string(), "mjpeg".to_string()],
            resolutions: vec!["1080p".to_string(), "1440p".to_string(), "4k".to_string()],
            fps: vec![30, 60],
            audio: true,
            battery: 100,
            min_zoom: 0.5,
            max_zoom: 10.0,
        }
    }
}

/// Real-time 1Hz telemetry status broadcast by the Android server over METADATA packets.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LiveStatusMetadata {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub fps: f32,
    #[serde(default)]
    pub bitrate: u64,
    #[serde(default)]
    pub resolution: String,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub battery: u32,
    #[serde(default)]
    pub charging: bool,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default)]
    pub dropped_frames: u64,
    #[serde(default)]
    pub zoom: f32,
    #[serde(default)]
    pub min_zoom: f32,
    #[serde(default)]
    pub max_zoom: f32,
    #[serde(default)]
    pub use_front_camera: bool,
    #[serde(default)]
    pub flash: bool,
    #[serde(default)]
    pub mirror: bool,
    #[serde(default)]
    pub focus_mode: String,
    #[serde(default)]
    pub focus_distance: f32,
    #[serde(default)]
    pub exposure_compensation: i32,
    #[serde(default)]
    pub white_balance: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_roundtrip_heartbeat() {
        let pkt = Packet::heartbeat();
        let mut buf = Vec::new();
        write_packet(&mut buf, &pkt).unwrap();
        assert_eq!(buf.len(), 14); // 14-byte header, 0 payload
        let mut cursor = Cursor::new(buf);
        let decoded = read_packet(&mut cursor).unwrap();
        assert_eq!(decoded.packet_type, PacketType::Heartbeat);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_roundtrip_video_frame_with_pts_and_flags() {
        let data = vec![0x00, 0x00, 0x00, 0x01, 0x65, 0xAB, 0xCD];
        let pkt = Packet::with_pts(
            PacketType::VideoFrame,
            PacketFlags::KEYFRAME | PacketFlags::MIRRORED,
            1_234_567_890,
            data.clone(),
        );
        let mut buf = Vec::new();
        write_packet(&mut buf, &pkt).unwrap();
        let mut cursor = Cursor::new(buf);
        let decoded = read_packet(&mut cursor).unwrap();
        assert_eq!(decoded.packet_type, PacketType::VideoFrame);
        assert!(decoded.is_keyframe());
        assert!(decoded.is_mirrored());
        assert_eq!(decoded.pts_us, 1_234_567_890);
        assert_eq!(decoded.payload, data);
    }

    #[test]
    fn test_roundtrip_control_cmd() {
        let cmd = ControlCommand::SetZoom { value: 2.5 };
        let pkt = Packet::control_cmd(&cmd).unwrap();
        let mut buf = Vec::new();
        write_packet(&mut buf, &pkt).unwrap();
        let mut cursor = Cursor::new(buf);
        let decoded = read_packet(&mut cursor).unwrap();
        assert_eq!(decoded.packet_type, PacketType::ControlCmd);
        let decoded_cmd: ControlCommand = serde_json::from_slice(&decoded.payload).unwrap();
        match decoded_cmd {
            ControlCommand::SetZoom { value } => assert!((value - 2.5).abs() < f32::EPSILON),
            _ => panic!("Expected SetZoom"),
        }
    }

    #[test]
    fn test_invalid_packet_type() {
        let mut buf = vec![0xFF, 0x00]; // invalid type 0xFF
        buf.extend_from_slice(&0i64.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let mut cursor = Cursor::new(buf);
        assert!(read_packet(&mut cursor).is_err());
    }

    #[test]
    fn test_oversized_payload_rejected() {
        let mut buf = Vec::new();
        buf.push(0x01); // VideoFrame
        buf.push(0x00); // Flags
        buf.write_i64::<LittleEndian>(0).unwrap();
        buf.write_u32::<LittleEndian>(MAX_PAYLOAD_SIZE + 1).unwrap();
        let mut cursor = Cursor::new(buf);
        assert!(read_packet(&mut cursor).is_err());
    }
}

