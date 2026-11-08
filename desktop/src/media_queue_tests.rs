use crate::media_queue::{BoundedAudioQueue, GateDecision, TimedMediaFrame, VideoGate};
use crate::protocol::v2::{MediaFlags, MediaKind, MediaPacket, MediaTimestamp, SessionId};

fn packet(
    kind: MediaKind,
    flags: MediaFlags,
    session: u64,
    sequence: u64,
    timestamp_ns: u64,
) -> TimedMediaFrame {
    TimedMediaFrame::from_packet(MediaPacket::new(
        kind,
        flags,
        SessionId(session),
        sequence,
        MediaTimestamp(timestamp_ns),
        vec![sequence as u8],
    ))
}

fn key(sequence: u64) -> TimedMediaFrame {
    packet(
        MediaKind::Video,
        MediaFlags::KEY_FRAME,
        1,
        sequence,
        sequence * 10,
    )
}

fn delta(sequence: u64) -> TimedMediaFrame {
    packet(
        MediaKind::Video,
        MediaFlags::NONE,
        1,
        sequence,
        sequence * 10,
    )
}

fn audio(sequence: u64, timestamp_ns: u64) -> TimedMediaFrame {
    packet(
        MediaKind::Audio,
        MediaFlags::NONE,
        1,
        sequence,
        timestamp_ns,
    )
}

#[test]
fn gate_starts_closed_and_opens_on_the_first_key_frame() {
    let mut gate = VideoGate::new();

    assert_eq!(gate.observe(&delta(1)), GateDecision::Drop);
    assert_eq!(gate.observe(&key(2)), GateDecision::Admit);
    assert_eq!(gate.observe(&delta(3)), GateDecision::Admit);
    assert!(!gate.is_awaiting_key_frame());
    assert_eq!(gate.stats().dropped_frames, 1);
}

#[test]
fn gate_drops_inter_frames_after_a_sequence_gap_until_the_next_key_frame() {
    let mut gate = VideoGate::new();
    gate.observe(&key(1));
    gate.observe(&delta(2));

    // Packet 3 was lost on the sender; 4 and 5 reference it.
    assert_eq!(gate.observe(&delta(4)), GateDecision::Drop);
    assert_eq!(gate.observe(&delta(5)), GateDecision::Drop);
    assert_eq!(gate.observe(&key(6)), GateDecision::Admit);
    assert_eq!(gate.observe(&delta(7)), GateDecision::Admit);

    let stats = gate.stats();
    assert_eq!(stats.sequence_gaps, 1);
    assert_eq!(stats.resyncs, 1);
    assert_eq!(stats.dropped_frames, 2);
}

#[test]
fn gate_admits_a_key_frame_that_follows_a_gap_in_the_same_unit() {
    let mut gate = VideoGate::new();
    gate.observe(&key(1));

    assert_eq!(gate.observe(&key(9)), GateDecision::Admit);
    assert_eq!(gate.stats().sequence_gaps, 7);
}

#[test]
fn gate_treats_a_discontinuity_as_a_resync_point() {
    let mut gate = VideoGate::new();
    gate.observe(&key(1));

    let discontinuity = packet(MediaKind::Video, MediaFlags::DISCONTINUITY, 1, 2, 20);
    assert_eq!(gate.observe(&discontinuity), GateDecision::Drop);

    let recovery = packet(
        MediaKind::Video,
        MediaFlags::KEY_FRAME | MediaFlags::DISCONTINUITY,
        1,
        3,
        30,
    );
    assert_eq!(gate.observe(&recovery), GateDecision::Admit);
}

#[test]
fn gate_resyncs_when_the_session_changes() {
    let mut gate = VideoGate::new();
    gate.observe(&key(1));

    let next_session = packet(MediaKind::Video, MediaFlags::NONE, 2, 1, 10);
    assert_eq!(gate.observe(&next_session), GateDecision::Drop);
    assert!(gate.is_awaiting_key_frame());
}

#[test]
fn gate_counts_a_resync_once_however_often_it_is_requested() {
    let mut gate = VideoGate::new();
    gate.observe(&key(1));

    gate.request_resync();
    gate.request_resync();

    assert_eq!(gate.stats().resyncs, 1);
}

#[test]
fn gate_never_blocks_legacy_v1_packets() {
    let mut gate = VideoGate::new();
    let legacy = packet(MediaKind::Video, MediaFlags::NONE, 0, 1, 0);

    assert_eq!(gate.observe(&legacy), GateDecision::Admit);
    assert!(
        gate.is_awaiting_key_frame(),
        "legacy frames must not change gate state"
    );
}

#[test]
fn audio_queue_discards_oldest_audio_outside_its_duration_budget() {
    let mut queue = BoundedAudioQueue::new(100);
    queue.push(audio(1, 0));
    queue.push(audio(2, 75));
    queue.push(audio(3, 150));

    assert_eq!(queue.len(), 2);
    assert_eq!(queue.stats().dropped_frames, 1);
}

#[test]
fn audio_queue_clears_stale_frames_on_session_change() {
    let mut queue = BoundedAudioQueue::new(1_000);
    queue.push(audio(1, 100));
    queue.push(packet(MediaKind::Audio, MediaFlags::NONE, 2, 1, 200));

    assert_eq!(queue.len(), 1);
    assert_eq!(queue.pop_front().unwrap().packet.session_id, SessionId(2));
    assert_eq!(queue.stats().dropped_frames, 1);
}
