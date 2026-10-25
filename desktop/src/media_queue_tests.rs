use crate::media_queue::{BoundedAudioQueue, LatestVideoQueue, TimedMediaFrame};
use crate::protocol::v2::{MediaFlags, MediaKind, MediaPacket, MediaTimestamp, SessionId};

fn video(sequence: u64, timestamp_ns: u64) -> TimedMediaFrame {
    TimedMediaFrame::from_packet(MediaPacket::new(
        MediaKind::Video,
        MediaFlags::NONE,
        SessionId(1),
        sequence,
        MediaTimestamp(timestamp_ns),
        vec![sequence as u8],
    ))
}

fn audio(sequence: u64, timestamp_ns: u64) -> TimedMediaFrame {
    TimedMediaFrame::from_packet(MediaPacket::new(
        MediaKind::Audio,
        MediaFlags::NONE,
        SessionId(1),
        sequence,
        MediaTimestamp(timestamp_ns),
        vec![sequence as u8],
    ))
}

#[test]
fn latest_video_queue_replaces_the_oldest_frame_at_capacity() {
    let mut queue = LatestVideoQueue::new(3);
    for sequence in 1..=4 {
        queue.push(video(sequence, sequence * 10));
    }

    assert_eq!(queue.len(), 3);
    assert_eq!(queue.pop_newest().unwrap().packet.sequence, 4);
    assert_eq!(queue.stats().dropped_stale_frames, 3);
}

#[test]
fn latest_video_queue_accounts_for_sequence_gaps_and_frame_age() {
    let mut queue = LatestVideoQueue::new(3);
    queue.push(video(10, 100));
    queue.push(video(13, 150));

    assert_eq!(queue.stats().sequence_gaps, 2);
    assert_eq!(queue.frame_age_ns(200), Some(50));
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
fn video_queue_clears_stale_frames_when_session_changes() {
    let mut queue = LatestVideoQueue::new(3);
    queue.push(video(20, 100));
    queue.push(TimedMediaFrame::from_packet(MediaPacket::new(
        MediaKind::Video,
        MediaFlags::NONE,
        SessionId(2),
        1,
        MediaTimestamp(200),
        vec![2],
    )));

    assert_eq!(queue.len(), 1);
    assert_eq!(queue.pop_newest().unwrap().packet.session_id, SessionId(2));
    assert_eq!(queue.stats().sequence_gaps, 0);
}

#[test]
fn video_configuration_packet_clears_stale_media_before_configuration() {
    let mut queue = LatestVideoQueue::new(3);
    queue.push(video(1, 100));
    queue.push(TimedMediaFrame::from_packet(MediaPacket::new(
        MediaKind::VideoConfig,
        MediaFlags::CONFIGURATION,
        SessionId(1),
        2,
        MediaTimestamp(200),
        vec![7, 8],
    )));

    assert_eq!(queue.len(), 1);
    assert_eq!(
        queue.pop_newest().unwrap().packet.kind,
        MediaKind::VideoConfig
    );
}

#[test]
fn audio_queue_clears_stale_frames_on_session_change() {
    let mut queue = BoundedAudioQueue::new(1_000);
    queue.push(audio(1, 100));
    let next_session = TimedMediaFrame::from_packet(MediaPacket::new(
        MediaKind::Audio,
        MediaFlags::NONE,
        SessionId(2),
        1,
        MediaTimestamp(200),
        vec![2],
    ));
    queue.push(next_session);

    assert_eq!(queue.len(), 1);
    assert_eq!(queue.pop_front().unwrap().packet.session_id, SessionId(2));
    assert_eq!(queue.stats().dropped_frames, 1);
}

#[test]
fn discontinuity_resets_receiver_queues_before_new_media() {
    let mut queue = LatestVideoQueue::new(3);
    queue.push(video(1, 10));
    queue.push(video(2, 20));
    let discontinuity = TimedMediaFrame::from_packet(MediaPacket::new(
        MediaKind::VideoConfig,
        MediaFlags::CONFIGURATION | MediaFlags::DISCONTINUITY,
        SessionId(2),
        1,
        MediaTimestamp(30),
        vec![1],
    ));

    queue.push(discontinuity);

    assert_eq!(queue.len(), 1);
    assert_eq!(queue.pop_newest().unwrap().packet.session_id, SessionId(2));
}
