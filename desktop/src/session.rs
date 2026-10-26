//! Capability negotiation and lifecycle rules shared by desktop transports.

use crate::protocol::v2::{SessionId, StreamConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionErrorCode {
    InvalidTransition,
    UnsupportedVersion,
    UnsupportedMode,
    Rejected,
    ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionError {
    pub code: SessionErrorCode,
    pub message: String,
}

impl SessionError {
    fn invalid_transition(from: &SessionPhase, event: &SessionEvent) -> Self {
        Self {
            code: SessionErrorCode::InvalidTransition,
            message: format!("cannot apply {event:?} while session is {from:?}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoMode {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub min_bitrate_bps: u64,
    pub max_bitrate_bps: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub protocol_versions: Vec<u8>,
    pub codecs: Vec<String>,
    pub video_modes: Vec<VideoMode>,
    pub audio_codecs: Vec<String>,
}

impl CapabilitySet {
    pub fn supports(&self, request: &StreamRequest) -> Result<(), SessionError> {
        if !self.protocol_versions.contains(&2) {
            return Err(SessionError {
                code: SessionErrorCode::UnsupportedVersion,
                message: "peer does not advertise protocol version 2".to_string(),
            });
        }
        if !self
            .codecs
            .iter()
            .any(|codec| codec.eq_ignore_ascii_case(&request.config.codec))
        {
            return Err(SessionError {
                code: SessionErrorCode::UnsupportedMode,
                message: format!("codec '{}' is not supported", request.config.codec),
            });
        }
        let Some(mode) = self.video_modes.iter().find(|mode| {
            mode.width == request.config.width
                && mode.height == request.config.height
                && mode.fps == request.config.fps
        }) else {
            return Err(SessionError {
                code: SessionErrorCode::UnsupportedMode,
                message: format!(
                    "{}x{}@{} is not supported",
                    request.config.width, request.config.height, request.config.fps
                ),
            });
        };
        if request.config.bitrate_bps < mode.min_bitrate_bps
            || request.config.bitrate_bps > mode.max_bitrate_bps
        {
            return Err(SessionError {
                code: SessionErrorCode::UnsupportedMode,
                message: format!(
                    "bitrate {} is outside the supported range {}..={}",
                    request.config.bitrate_bps, mode.min_bitrate_bps, mode.max_bitrate_bps
                ),
            });
        }
        if request.audio_enabled && self.audio_codecs.is_empty() {
            return Err(SessionError {
                code: SessionErrorCode::UnsupportedMode,
                message: "audio was requested but no audio codec is supported".to_string(),
            });
        }
        Ok(())
    }
}

impl CapabilitySet {
    /// Adapts a desired stream to what the phone actually offers.
    ///
    /// The phone is authoritative: if the exact codec or mode is unavailable
    /// the closest one is chosen instead of failing the whole connection. The
    /// result is guaranteed to satisfy [`CapabilitySet::supports`].
    pub fn resolve(&self, desired: &StreamRequest) -> Result<StreamRequest, SessionError> {
        if !self.protocol_versions.contains(&2) {
            return Err(SessionError {
                code: SessionErrorCode::UnsupportedVersion,
                message: "peer does not advertise protocol version 2".to_string(),
            });
        }
        let unsupported = |message: &str| SessionError {
            code: SessionErrorCode::UnsupportedMode,
            message: message.to_string(),
        };

        let wanted_codec = &desired.config.codec;
        let codec = self
            .codecs
            .iter()
            .find(|codec| codec.eq_ignore_ascii_case(wanted_codec))
            .or_else(|| {
                self.codecs
                    .iter()
                    .find(|codec| codec.eq_ignore_ascii_case("h264"))
            })
            .or_else(|| self.codecs.first())
            .ok_or_else(|| unsupported("the phone advertises no video codecs"))?;

        let wanted_pixels = u64::from(desired.config.width) * u64::from(desired.config.height);
        let mode = self
            .video_modes
            .iter()
            .min_by_key(|mode| {
                let pixels = u64::from(mode.width) * u64::from(mode.height);
                (
                    pixels.abs_diff(wanted_pixels),
                    mode.fps.abs_diff(desired.config.fps),
                )
            })
            .ok_or_else(|| unsupported("the phone advertises no video modes"))?;

        let exact = mode.width == desired.config.width
            && mode.height == desired.config.height
            && mode.fps == desired.config.fps
            && codec.eq_ignore_ascii_case(wanted_codec);
        let target_bitrate = if exact {
            desired.config.bitrate_bps
        } else {
            recommended_bitrate(mode.width, mode.height, mode.fps, codec)
        };

        Ok(StreamRequest {
            config: StreamConfig {
                codec: codec.clone(),
                width: mode.width,
                height: mode.height,
                fps: mode.fps,
                bitrate_bps: target_bitrate.clamp(mode.min_bitrate_bps, mode.max_bitrate_bps),
                ..desired.config.clone()
            },
            audio_enabled: desired.audio_enabled && !self.audio_codecs.is_empty(),
        })
    }
}

/// A sensible target bitrate for a mode, in bits per second.
///
/// Based on bits per pixel per frame: H.265 reaches the same quality as H.264
/// at roughly 60% of the rate, while MJPEG (no inter-frame prediction) needs
/// several times more.
pub fn recommended_bitrate(width: u32, height: u32, fps: u32, codec: &str) -> u64 {
    let bits_per_pixel = match codec.to_ascii_lowercase().as_str() {
        "h265" | "hevc" => 0.06,
        "mjpeg" => 0.35,
        _ => 0.10,
    };
    (f64::from(width) * f64::from(height) * f64::from(fps) * bits_per_pixel) as u64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamRequest {
    pub config: StreamConfig,
    pub audio_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegotiatedStream {
    pub session_id: SessionId,
    pub config: StreamConfig,
    pub audio_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionPhase {
    Idle,
    Connecting,
    Negotiating,
    AwaitingStartAck,
    Streaming(NegotiatedStream),
    AwaitingReconfigureAck,
    Reconnecting,
    Stopped,
    Failed(SessionError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    ConnectRequested,
    TransportConnected,
    CapabilitiesReceived(CapabilitySet),
    StartRequested(StreamRequest),
    StartAcknowledged(NegotiatedStream),
    ReconfigureRequested(StreamRequest),
    ReconfigureAcknowledged(NegotiatedStream),
    ConnectionLost,
    StopRequested,
    Rejected(SessionError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEffect {
    SendStart(StreamRequest),
    SendReconfigure(StreamRequest),
    StartMedia(NegotiatedStream),
    ReconfigureMedia(NegotiatedStream),
    ClearMediaQueues,
}

/// Control-plane messages carried as JSON metadata while the media path uses
/// the timestamped binary v2 envelope. `type` is stable across Kotlin/Gson
/// and Rust/Serde implementations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SessionMessage {
    Hello {
        protocol_versions: Vec<u8>,
        client: String,
    },
    Capabilities(CapabilitySet),
    Start(StreamRequest),
    StreamAccepted(NegotiatedStream),
    StreamRejected(SessionError),
    Reconfigure(StreamRequest),
    Reconfigured(NegotiatedStream),
}

/// A pure state machine. Transports execute its effects, which keeps state
/// transitions testable and prevents UI state from diverging from the stream.
#[derive(Debug, Clone)]
pub struct SessionMachine {
    phase: SessionPhase,
    capabilities: Option<CapabilitySet>,
    pending_request: Option<StreamRequest>,
    pending_stream: Option<NegotiatedStream>,
    queue_generation: u64,
}

impl Default for SessionMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMachine {
    pub fn new() -> Self {
        Self {
            phase: SessionPhase::Idle,
            capabilities: None,
            pending_request: None,
            pending_stream: None,
            queue_generation: 0,
        }
    }

    pub fn phase(&self) -> &SessionPhase {
        &self.phase
    }

    pub fn capabilities(&self) -> Option<&CapabilitySet> {
        self.capabilities.as_ref()
    }

    pub fn pending_stream(&self) -> Option<&NegotiatedStream> {
        self.pending_stream.as_ref()
    }

    pub fn queue_generation(&self) -> u64 {
        self.queue_generation
    }

    pub fn apply(&mut self, event: SessionEvent) -> Result<Vec<SessionEffect>, SessionError> {
        match (&self.phase, &event) {
            (
                SessionPhase::Idle | SessionPhase::Stopped | SessionPhase::Reconnecting,
                SessionEvent::ConnectRequested,
            ) => {
                self.phase = SessionPhase::Connecting;
                Ok(vec![])
            }
            (SessionPhase::Connecting, SessionEvent::TransportConnected) => {
                self.phase = SessionPhase::Negotiating;
                Ok(vec![])
            }
            (SessionPhase::Negotiating, SessionEvent::CapabilitiesReceived(capabilities)) => {
                if !capabilities.protocol_versions.contains(&2) {
                    return self.reject(SessionError {
                        code: SessionErrorCode::UnsupportedVersion,
                        message: "peer does not support protocol version 2".to_string(),
                    });
                }
                self.capabilities = Some(capabilities.clone());
                Ok(vec![])
            }
            (SessionPhase::Negotiating, SessionEvent::StartRequested(request)) => {
                self.validate_request(request)?;
                self.pending_request = Some(request.clone());
                self.phase = SessionPhase::AwaitingStartAck;
                Ok(vec![SessionEffect::SendStart(request.clone())])
            }
            (SessionPhase::AwaitingStartAck, SessionEvent::StartAcknowledged(stream)) => {
                self.validate_acknowledged_stream(stream)?;
                self.pending_stream = Some(stream.clone());
                self.pending_request = None;
                self.phase = SessionPhase::Streaming(stream.clone());
                Ok(vec![SessionEffect::StartMedia(stream.clone())])
            }
            (SessionPhase::Streaming(_), SessionEvent::ReconfigureRequested(request)) => {
                self.validate_request(request)?;
                self.pending_request = Some(request.clone());
                self.phase = SessionPhase::AwaitingReconfigureAck;
                Ok(vec![SessionEffect::SendReconfigure(request.clone())])
            }
            (
                SessionPhase::AwaitingReconfigureAck,
                SessionEvent::ReconfigureAcknowledged(stream),
            ) => {
                self.validate_acknowledged_stream(stream)?;
                self.pending_stream = Some(stream.clone());
                self.pending_request = None;
                self.queue_generation += 1;
                self.phase = SessionPhase::Streaming(stream.clone());
                Ok(vec![
                    SessionEffect::ClearMediaQueues,
                    SessionEffect::ReconfigureMedia(stream.clone()),
                ])
            }
            (_, SessionEvent::ConnectionLost) => {
                self.capabilities = None;
                self.pending_request = None;
                self.pending_stream = None;
                self.queue_generation += 1;
                self.phase = SessionPhase::Reconnecting;
                Ok(vec![SessionEffect::ClearMediaQueues])
            }
            (_, SessionEvent::StopRequested) => {
                self.pending_request = None;
                self.pending_stream = None;
                self.phase = SessionPhase::Stopped;
                Ok(vec![SessionEffect::ClearMediaQueues])
            }
            (_, SessionEvent::Rejected(error)) => self.reject(error.clone()),
            _ => Err(SessionError::invalid_transition(&self.phase, &event)),
        }
    }

    fn validate_request(&self, request: &StreamRequest) -> Result<(), SessionError> {
        self.capabilities
            .as_ref()
            .ok_or_else(|| SessionError {
                code: SessionErrorCode::InvalidTransition,
                message: "cannot request a stream before capabilities arrive".to_string(),
            })?
            .supports(request)
    }

    fn validate_acknowledged_stream(&self, stream: &NegotiatedStream) -> Result<(), SessionError> {
        let Some(request) = self.pending_request.as_ref() else {
            return Err(SessionError {
                code: SessionErrorCode::InvalidTransition,
                message: "received a stream acknowledgement without a pending request".to_string(),
            });
        };
        if request.config != stream.config || request.audio_enabled != stream.audio_enabled {
            return Err(SessionError {
                code: SessionErrorCode::ProtocolError,
                message: "peer acknowledged a stream different from the requested mode".to_string(),
            });
        }
        self.validate_request(request)
    }

    fn reject(&mut self, error: SessionError) -> Result<Vec<SessionEffect>, SessionError> {
        self.phase = SessionPhase::Failed(error.clone());
        Err(error)
    }
}
