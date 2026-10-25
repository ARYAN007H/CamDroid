//! Version 2 of the CamDroid media envelope.
//!
//! The envelope is intentionally separate from the legacy v1 packet framing.
//! It gives every media unit a session identity, sequence number and capture
//! timestamp, while retaining typed JSON control payloads.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// `CMD2` in ASCII. A distinct magic makes protocol mismatches fail early.
pub const V2_MAGIC: [u8; 4] = *b"CMD2";
pub const V2_VERSION: u8 = 2;
pub const V2_HEADER_LEN: usize = 36;
pub const V2_MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024;

const VERSION_OFFSET: usize = 4;
const KIND_OFFSET: usize = 5;
const FLAGS_OFFSET: usize = 6;
const SESSION_OFFSET: usize = 8;
const SEQUENCE_OFFSET: usize = 16;
const TIMESTAMP_OFFSET: usize = 24;
const PAYLOAD_LENGTH_OFFSET: usize = 32;

/// Identifies one negotiated connection. Its bits are encoded little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

/// A monotonic capture time in nanoseconds, measured by the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MediaTimestamp(pub u64);

/// Packet class values intentionally retain the legacy packet-kind mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MediaKind {
    Video = 0x01,
    Audio = 0x02,
    Control = 0x03,
    Metadata = 0x04,
    Heartbeat = 0x05,
    VideoConfig = 0x06,
    AudioConfig = 0x07,
}

impl TryFrom<u8> for MediaKind {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0x01 => Ok(Self::Video),
            0x02 => Ok(Self::Audio),
            0x03 => Ok(Self::Control),
            0x04 => Ok(Self::Metadata),
            0x05 => Ok(Self::Heartbeat),
            0x06 => Ok(Self::VideoConfig),
            0x07 => Ok(Self::AudioConfig),
            _ => bail!("unsupported media kind: 0x{value:02x}"),
        }
    }
}

/// Semantics that affect decoder and queue handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MediaFlags(pub u16);

impl MediaFlags {
    pub const NONE: Self = Self(0);
    pub const KEY_FRAME: Self = Self(1 << 0);
    pub const CONFIGURATION: Self = Self(1 << 1);
    pub const DISCONTINUITY: Self = Self(1 << 2);
    const KNOWN_BITS: u16 = Self::KEY_FRAME.0 | Self::CONFIGURATION.0 | Self::DISCONTINUITY.0;

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    fn from_bits(bits: u16) -> Result<Self> {
        if bits & !Self::KNOWN_BITS != 0 {
            bail!("unsupported media flags: 0x{bits:04x}");
        }
        Ok(Self(bits))
    }
}

impl std::ops::BitOr for MediaFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

/// A configuration chosen by capability negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamConfig {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioStreamConfig>,
}

/// Audio configuration paired with a negotiated video stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStreamConfig {
    pub codec: String,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub bitrate_bps: u64,
}

/// JSON payload wrapper used for typed v2 control messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPayload<T> {
    pub message: T,
}

/// One fully framed v2 unit. Payload ownership prevents a caller from
/// reusing an encoder buffer while a sender still needs the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPacket {
    pub kind: MediaKind,
    pub flags: MediaFlags,
    pub session_id: SessionId,
    pub sequence: u64,
    pub capture_timestamp: MediaTimestamp,
    pub payload: Vec<u8>,
}

impl MediaPacket {
    pub fn new(
        kind: MediaKind,
        flags: MediaFlags,
        session_id: SessionId,
        sequence: u64,
        capture_timestamp: MediaTimestamp,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            kind,
            flags,
            session_id,
            sequence,
            capture_timestamp,
            payload,
        }
    }

    /// Serializes a typed control message as a v2 control packet.
    pub fn control<T: Serialize>(
        session_id: SessionId,
        sequence: u64,
        capture_timestamp: MediaTimestamp,
        message: &T,
    ) -> Result<Self> {
        let payload = serde_json::to_vec(&ControlPayload { message })
            .context("failed to encode v2 control JSON")?;
        Ok(Self::new(
            MediaKind::Control,
            MediaFlags::NONE,
            session_id,
            sequence,
            capture_timestamp,
            payload,
        ))
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        validate_payload_length(self.payload.len())?;
        let mut bytes = vec![0; V2_HEADER_LEN + self.payload.len()];
        bytes[..4].copy_from_slice(&V2_MAGIC);
        bytes[VERSION_OFFSET] = V2_VERSION;
        bytes[KIND_OFFSET] = self.kind as u8;
        bytes[FLAGS_OFFSET..SESSION_OFFSET].copy_from_slice(&self.flags.0.to_le_bytes());
        bytes[SESSION_OFFSET..SEQUENCE_OFFSET].copy_from_slice(&self.session_id.0.to_le_bytes());
        bytes[SEQUENCE_OFFSET..TIMESTAMP_OFFSET].copy_from_slice(&self.sequence.to_le_bytes());
        bytes[TIMESTAMP_OFFSET..PAYLOAD_LENGTH_OFFSET]
            .copy_from_slice(&self.capture_timestamp.0.to_le_bytes());
        bytes[PAYLOAD_LENGTH_OFFSET..V2_HEADER_LEN]
            .copy_from_slice(&(self.payload.len() as u32).to_le_bytes());
        bytes[V2_HEADER_LEN..].copy_from_slice(&self.payload);
        Ok(bytes)
    }

    /// Decodes exactly one complete v2 unit from an in-memory transport frame.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let (kind, flags, session_id, sequence, capture_timestamp, payload_length) =
            parse_header(bytes)?;
        let expected_len = V2_HEADER_LEN + payload_length;
        if bytes.len() < expected_len {
            bail!(
                "truncated v2 payload: declared {payload_length} bytes, received {}",
                bytes.len().saturating_sub(V2_HEADER_LEN)
            );
        }
        if bytes.len() > expected_len {
            bail!("v2 frame has {} trailing bytes", bytes.len() - expected_len);
        }
        Ok(Self::new(
            kind,
            flags,
            session_id,
            sequence,
            capture_timestamp,
            bytes[V2_HEADER_LEN..].to_vec(),
        ))
    }

    /// Reads one packet from a TCP byte stream without allocating until the
    /// length field has passed the configured safety bound.
    pub fn read_from(reader: &mut impl Read) -> Result<Self> {
        let mut header = [0_u8; V2_HEADER_LEN];
        reader
            .read_exact(&mut header)
            .context("failed to read v2 packet header")?;
        let (kind, flags, session_id, sequence, capture_timestamp, payload_length) =
            parse_header(&header)?;
        let mut payload = vec![0; payload_length];
        reader
            .read_exact(&mut payload)
            .context("failed to read v2 packet payload")?;
        Ok(Self::new(
            kind,
            flags,
            session_id,
            sequence,
            capture_timestamp,
            payload,
        ))
    }

    /// Reads the remainder of an envelope after a transport has already
    /// consumed the four-byte `CMD2` magic while selecting a wire format.
    pub fn read_after_magic(reader: &mut impl Read) -> Result<Self> {
        let mut header = [0_u8; V2_HEADER_LEN];
        header[..4].copy_from_slice(&V2_MAGIC);
        reader
            .read_exact(&mut header[4..])
            .context("failed to read v2 packet header")?;
        let (kind, flags, session_id, sequence, capture_timestamp, payload_length) =
            parse_header(&header)?;
        let mut payload = vec![0; payload_length];
        reader
            .read_exact(&mut payload)
            .context("failed to read v2 packet payload")?;
        Ok(Self::new(
            kind,
            flags,
            session_id,
            sequence,
            capture_timestamp,
            payload,
        ))
    }

    pub fn write_to(&self, writer: &mut impl Write) -> Result<()> {
        let bytes = self.encode()?;
        writer
            .write_all(&bytes)
            .context("failed to write v2 packet")?;
        Ok(())
    }
}

fn parse_header(
    bytes: &[u8],
) -> Result<(MediaKind, MediaFlags, SessionId, u64, MediaTimestamp, usize)> {
    if bytes.len() < V2_HEADER_LEN {
        bail!(
            "truncated v2 header: expected {V2_HEADER_LEN} bytes, received {}",
            bytes.len()
        );
    }
    if bytes[..4] != V2_MAGIC {
        bail!("invalid v2 protocol magic");
    }
    let version = bytes[VERSION_OFFSET];
    if version != V2_VERSION {
        bail!("unsupported protocol version: {version}");
    }
    let kind = MediaKind::try_from(bytes[KIND_OFFSET])?;
    let flags = MediaFlags::from_bits(u16::from_le_bytes(
        bytes[FLAGS_OFFSET..SESSION_OFFSET].try_into().unwrap(),
    ))?;
    let session_id = SessionId(u64::from_le_bytes(
        bytes[SESSION_OFFSET..SEQUENCE_OFFSET].try_into().unwrap(),
    ));
    let sequence = u64::from_le_bytes(bytes[SEQUENCE_OFFSET..TIMESTAMP_OFFSET].try_into().unwrap());
    let capture_timestamp = MediaTimestamp(u64::from_le_bytes(
        bytes[TIMESTAMP_OFFSET..PAYLOAD_LENGTH_OFFSET]
            .try_into()
            .unwrap(),
    ));
    let payload_length = u32::from_le_bytes(
        bytes[PAYLOAD_LENGTH_OFFSET..V2_HEADER_LEN]
            .try_into()
            .unwrap(),
    ) as usize;
    validate_payload_length(payload_length)?;
    Ok((
        kind,
        flags,
        session_id,
        sequence,
        capture_timestamp,
        payload_length,
    ))
}

fn validate_payload_length(payload_length: usize) -> Result<()> {
    if payload_length > V2_MAX_PAYLOAD_SIZE {
        bail!("v2 payload too large: {payload_length} bytes (max {V2_MAX_PAYLOAD_SIZE})");
    }
    Ok(())
}
