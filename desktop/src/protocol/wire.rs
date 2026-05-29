// CamDroid Wire Protocol
//
// Binary protocol for streaming video, audio, and control commands
// between the Android phone (server) and the desktop client.
//
// Packet format: [Type: 1 byte] [Length: 4 bytes LE] [Payload: N bytes]

use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Maximum allowed payload size (16 MB) to prevent memory exhaustion
/// from malformed or corrupted packets.
const MAX_PAYLOAD_SIZE: u32 = 16 * 1024 * 1024;

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

/// A single protocol packet with its type and payload.
#[derive(Debug, Clone)]
pub struct Packet {
    pub packet_type: PacketType,
    pub payload: Vec<u8>,
}

impl Packet {
    /// Create a new packet with the given type and payload data.
    pub fn new(packet_type: PacketType, payload: Vec<u8>) -> Self {
        Self {
            packet_type,
            payload,
        }
    }

    /// Create an empty heartbeat packet.
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

/// Read exactly one complete packet from the given reader.
///
/// Handles partial TCP reads by looping until all bytes are received.
/// Returns an error on connection close (zero-byte read) or I/O failure.
pub fn read_packet(reader: &mut impl Read) -> Result<Packet> {
    // Read the 1-byte type field
    let type_byte = reader
        .read_u8()
        .context("Failed to read packet type (connection closed?)")?;
    let packet_type = PacketType::from_byte(type_byte)?;

    // Read the 4-byte payload length (little-endian)
    let length = reader
        .read_u32::<LittleEndian>()
        .context("Failed to read packet length")?;

    // Validate payload size to prevent memory exhaustion
    if length > MAX_PAYLOAD_SIZE {
        bail!(
            "Packet payload too large: {} bytes (max {})",
            length,
            MAX_PAYLOAD_SIZE
        );
    }

    // Read the payload — read_exact handles partial reads internally
    let mut payload = vec![0u8; length as usize];
    if length > 0 {
        reader
            .read_exact(&mut payload)
            .context("Failed to read packet payload (connection closed mid-packet?)")?;
    }

    Ok(Packet {
        packet_type,
        payload,
    })
}

/// Write a complete packet to the given writer.
///
/// Serializes the type byte, payload length, and payload data.
/// Flushes the writer to ensure the packet is sent immediately.
pub fn write_packet(writer: &mut impl Write, packet: &Packet) -> Result<()> {
    writer
        .write_u8(packet.packet_type as u8)
        .context("Failed to write packet type")?;
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
    /// Set the camera zoom ratio (1.0 = no zoom).
    SetZoom { value: f32 },
    /// Set focus mode and optional manual distance.
    SetFocus {
        mode: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        distance: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        x: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        y: Option<f32>,
    },
    /// Set exposure compensation in EV steps.
    SetExposure { compensation: i32 },
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

/// Metadata sent during the initial handshake or as status updates.
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
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            version: "unknown".to_string(),
            device: "unknown".to_string(),
            codecs: vec!["h264".to_string()],
            resolutions: vec!["1080p".to_string()],
            fps: vec![30, 60],
            audio: true,
            battery: 100,
        }
    }
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
        let mut cursor = Cursor::new(buf);
        let decoded = read_packet(&mut cursor).unwrap();
        assert_eq!(decoded.packet_type, PacketType::Heartbeat);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_roundtrip_video_frame() {
        let data = vec![0x00, 0x00, 0x00, 0x01, 0x65, 0xAB, 0xCD];
        let pkt = Packet::new(PacketType::VideoFrame, data.clone());
        let mut buf = Vec::new();
        write_packet(&mut buf, &pkt).unwrap();
        let mut cursor = Cursor::new(buf);
        let decoded = read_packet(&mut cursor).unwrap();
        assert_eq!(decoded.packet_type, PacketType::VideoFrame);
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
        let buf = vec![0xFF, 0x00, 0x00, 0x00, 0x00]; // invalid type 0xFF
        let mut cursor = Cursor::new(buf);
        assert!(read_packet(&mut cursor).is_err());
    }

    #[test]
    fn test_oversized_payload_rejected() {
        let mut buf = Vec::new();
        buf.push(0x01); // VideoFrame
        buf.write_u32::<LittleEndian>(MAX_PAYLOAD_SIZE + 1).unwrap();
        let mut cursor = Cursor::new(buf);
        assert!(read_packet(&mut cursor).is_err());
    }
}
