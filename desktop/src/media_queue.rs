//! Bounded, timestamp-aware receiver queues and the video admission gate.

use crate::protocol::v2::{MediaFlags, MediaKind, MediaPacket, SessionId};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct TimedMediaFrame {
    pub packet: MediaPacket,
}

impl TimedMediaFrame {
    pub fn from_packet(packet: MediaPacket) -> Self {
        Self { packet }
    }

    pub fn is_discontinuity(&self) -> bool {
        self.packet.flags.contains(MediaFlags::DISCONTINUITY)
    }

    pub fn is_key_frame(&self) -> bool {
        self.packet.flags.contains(MediaFlags::KEY_FRAME)
    }
}

/// Whether an encoded video unit may be handed to the decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Admit,
    Drop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VideoGateStats {
    /// Units discarded while waiting for the next key frame.
    pub dropped_frames: u64,
    /// Missing video sequence numbers observed within a session.
    pub sequence_gaps: u64,
    /// Number of times the gate stopped admitting inter-coded frames.
    pub resyncs: u64,
}

/// Protects the decoder's reference chain.
///
/// Inter-coded video (H.264/H.265 P- and B-frames) can only be decoded
/// against the frames before it. Discarding an arbitrary frame to reduce
/// latency therefore corrupts the picture until the next key frame. The gate
/// makes the only safe choice: once continuity is lost for any reason it
/// drops everything until a key frame arrives, then resumes cleanly.
///
/// Legacy v1 packets (session id 0) carry no flags and bypass the gate.
#[derive(Debug)]
pub struct VideoGate {
    awaiting_key_frame: bool,
    last_sequence: Option<u64>,
    session_id: Option<SessionId>,
    stats: VideoGateStats,
}

impl Default for VideoGate {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoGate {
    /// A new stream always begins on a key frame, so the gate starts closed.
    pub fn new() -> Self {
        Self {
            awaiting_key_frame: true,
            last_sequence: None,
            session_id: None,
            stats: VideoGateStats::default(),
        }
    }

    pub fn observe(&mut self, frame: &TimedMediaFrame) -> GateDecision {
        let packet = &frame.packet;
        if packet.session_id == SessionId(0) {
            return GateDecision::Admit;
        }

        if self.session_id != Some(packet.session_id) {
            self.session_id = Some(packet.session_id);
            self.last_sequence = None;
            self.request_resync();
        }
        if frame.is_discontinuity() {
            self.request_resync();
        }
        if let Some(previous) = self.last_sequence {
            if packet.sequence > previous + 1 {
                self.stats.sequence_gaps += packet.sequence - previous - 1;
                self.request_resync();
            }
        }
        self.last_sequence = Some(packet.sequence);

        if !self.awaiting_key_frame {
            return GateDecision::Admit;
        }
        if frame.is_key_frame() {
            self.awaiting_key_frame = false;
            GateDecision::Admit
        } else {
            self.stats.dropped_frames += 1;
            GateDecision::Drop
        }
    }

    /// Stops admitting frames until the next key frame. Idempotent: only the
    /// transition from open to closed is counted as a resync.
    pub fn request_resync(&mut self) {
        if !self.awaiting_key_frame {
            self.awaiting_key_frame = true;
            self.stats.resyncs += 1;
        }
    }

    pub fn is_awaiting_key_frame(&self) -> bool {
        self.awaiting_key_frame
    }

    pub fn stats(&self) -> VideoGateStats {
        self.stats
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioQueueStats {
    pub dropped_frames: u64,
}

pub struct BoundedAudioQueue {
    max_duration_ns: u64,
    frames: VecDeque<TimedMediaFrame>,
    stats: AudioQueueStats,
    session_id: Option<SessionId>,
}

impl BoundedAudioQueue {
    pub fn new(max_duration_ns: u64) -> Self {
        Self {
            max_duration_ns,
            frames: VecDeque::new(),
            stats: AudioQueueStats::default(),
            session_id: None,
        }
    }

    pub fn push(&mut self, frame: TimedMediaFrame) {
        if frame.is_discontinuity()
            || frame.packet.kind == MediaKind::AudioConfig
            || self
                .session_id
                .is_some_and(|session_id| session_id != frame.packet.session_id)
        {
            self.stats.dropped_frames += self.frames.len() as u64;
            self.frames.clear();
        }
        self.session_id = Some(frame.packet.session_id);
        self.frames.push_back(frame);
        while self.duration_ns() > self.max_duration_ns {
            self.frames.pop_front();
            self.stats.dropped_frames += 1;
        }
    }

    pub fn pop_front(&mut self) -> Option<TimedMediaFrame> {
        self.frames.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn stats(&self) -> AudioQueueStats {
        self.stats
    }

    fn duration_ns(&self) -> u64 {
        match (self.frames.front(), self.frames.back()) {
            (Some(first), Some(last)) => last
                .packet
                .capture_timestamp
                .0
                .saturating_sub(first.packet.capture_timestamp.0),
            _ => 0,
        }
    }
}
