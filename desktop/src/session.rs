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
