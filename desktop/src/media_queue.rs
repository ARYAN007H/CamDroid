//! Bounded, timestamp-aware receiver queues.

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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VideoQueueStats {
    pub dropped_stale_frames: u64,
    pub sequence_gaps: u64,
}

/// Keeps a small decode backlog and hands consumers the freshest frame.
pub struct LatestVideoQueue {
    capacity: usize,
    frames: VecDeque<TimedMediaFrame>,
    stats: VideoQueueStats,
    last_sequence: Option<u64>,
    session_id: Option<SessionId>,
}

impl LatestVideoQueue {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            capacity,
            frames: VecDeque::with_capacity(capacity),
            stats: VideoQueueStats::default(),
            last_sequence: None,
            session_id: None,
        }
    }

    pub fn push(&mut self, frame: TimedMediaFrame) {
        if frame.is_discontinuity()
            || frame.packet.kind == MediaKind::VideoConfig
            || self
                .session_id
                .is_some_and(|session_id| session_id != frame.packet.session_id)
        {
            self.reset();
        }
        if let Some(previous) = self.last_sequence {
            if frame.packet.sequence > previous + 1 {
                self.stats.sequence_gaps += frame.packet.sequence - previous - 1;
            }
        }
        self.last_sequence = Some(frame.packet.sequence);
        self.session_id = Some(frame.packet.session_id);
        if self.frames.len() == self.capacity {
            self.frames.pop_front();
            self.stats.dropped_stale_frames += 1;
        }
        self.frames.push_back(frame);
    }

    pub fn pop_newest(&mut self) -> Option<TimedMediaFrame> {
        let newest = self.frames.pop_back()?;
        self.stats.dropped_stale_frames += self.frames.len() as u64;
        self.frames.clear();
        Some(newest)
    }

    pub fn frame_age_ns(&self, now_ns: u64) -> Option<u64> {
        self.frames
            .back()
            .map(|frame| now_ns.saturating_sub(frame.packet.capture_timestamp.0))
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn stats(&self) -> VideoQueueStats {
        self.stats
    }
    pub fn reset(&mut self) {
        self.frames.clear();
        self.last_sequence = None;
        self.session_id = None;
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
