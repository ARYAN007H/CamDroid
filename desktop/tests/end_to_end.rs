//! End-to-end tests: a fake phone speaks protocol v2 over a real TCP socket
//! and streams a real H.264 clip into the real pipeline (TCP reader, video
//! gate, FFmpeg decoder, render stage, preview hub).
//!
//! The clip (`tests/fixtures/red_then_blue_320x180.h264`) is 60 frames at
//! 30 fps: 30 red, then 30 blue, one key frame every 15 frames, no B-frames.
//! The colour change lands on a key frame, so "what colour is the preview"
//! is an observable signal for both normal playback and loss recovery.
//!
//! Regenerate the fixture with:
//!   ffmpeg -f lavfi -i color=c=0xC83232:s=320x180:r=30:d=1 \
//!          -f lavfi -i color=c=0x3232C8:s=320x180:r=30:d=1 \
//!          -filter_complex "[0][1]concat=n=2:v=1[v]" -map "[v]" \
//!          -c:v libx264 -preset veryfast -profile:v baseline -g 15 \
//!          -keyint_min 15 -sc_threshold 0 -bf 0 -pix_fmt yuv420p \
//!          -f h264 red_then_blue_320x180.h264

use camdroid_client::protocol::v2::{
    MediaFlags, MediaKind, MediaPacket, MediaTimestamp, SessionId,
};
use camdroid_client::protocol::{read_packet, PacketType};
use camdroid_client::session::{CapabilitySet, NegotiatedStream, SessionMessage, VideoMode};
use camdroid_client::{
    CamdroidSession, ConnectionMode, OutputMode, PreviewFrame, PreviewWait, SessionConfig,
};
use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FIXTURE: &[u8] = include_bytes!("fixtures/red_then_blue_320x180.h264");
const SESSION: SessionId = SessionId(7);
const FRAME_INTERVAL: Duration = Duration::from_millis(10);
const WAIT: Duration = Duration::from_secs(8);

static FFMPEG_INIT: Once = Once::new();

fn init_ffmpeg() {
    FFMPEG_INIT.call_once(|| CamdroidSession::init().expect("FFmpeg initialises"));
}

// ---------------------------------------------------------------------------
// Annex B parsing
// ---------------------------------------------------------------------------

struct Clip {
    /// SPS + PPS, sent once as the codec configuration.
    config: Vec<u8>,
    frames: Vec<ClipFrame>,
}

struct ClipFrame {
    data: Vec<u8>,
    key: bool,
}

/// Splits an Annex B stream into (nal_type, bytes-with-4-byte-start-code).
fn split_nals(data: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut nals = Vec::new();
    for (index, &start) in starts.iter().enumerate() {
        let mut end = starts.get(index + 1).map_or(data.len(), |next| next - 3);
        // A 4-byte start code leaves one extra zero on the previous NAL.
        while end > start && data[end - 1] == 0 && index + 1 < starts.len() {
            end -= 1;
        }
        let mut nal = vec![0, 0, 0, 1];
        nal.extend_from_slice(&data[start..end]);
        nals.push((data[start] & 0x1f, nal));
    }
    nals
}

fn load_clip() -> Clip {
    let mut config = Vec::new();
    let mut frames = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut have_config = false;
    for (nal_type, nal) in split_nals(FIXTURE) {
        match nal_type {
            7 | 8 => {
                // Android sends parameter sets once, as configuration.
                if !have_config {
                    config.extend_from_slice(&nal);
                }
            }
            1 | 5 => {
                have_config |= !config.is_empty();
                pending.extend_from_slice(&nal);
                frames.push(ClipFrame {
                    data: std::mem::take(&mut pending),
                    key: nal_type == 5,
                });
            }
            _ => pending.extend_from_slice(&nal),
        }
    }
    Clip { config, frames }
}

// ---------------------------------------------------------------------------
// The fake phone
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct PhoneScript {
    /// Video packet sequence number to silently drop (a loss on the phone side).
    drop_video_sequence: Option<u64>,
    /// Close the socket after sending this many frames.
    hang_up_after: Option<usize>,
}

impl PhoneScript {
    const CLEAN: Self = Self {
        drop_video_sequence: None,
        hang_up_after: None,
    };
}

struct Phone {
    port: u16,
    /// Names of the `cmd` field of every control command the desktop sent.
    commands: Arc<Mutex<Vec<String>>>,
    finished: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Phone {
    fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }

    fn streamed_everything(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn session_packet(message: &SessionMessage) -> MediaPacket {
    MediaPacket::new(
        MediaKind::Metadata,
        MediaFlags::NONE,
        SessionId(0),
        0,
        MediaTimestamp(0),
        serde_json::to_vec(message).unwrap(),
    )
}

fn spawn_phone(script: PhoneScript) -> Phone {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let commands = Arc::new(Mutex::new(Vec::new()));
    let finished = Arc::new(AtomicBool::new(false));

    let thread_commands = Arc::clone(&commands);
    let thread_finished = Arc::clone(&finished);
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_nodelay(true).unwrap();
        let mut reader = stream.try_clone().unwrap();

        // Handshake: Hello -> Capabilities -> Start -> StreamAccepted.
        let hello = read_packet(&mut reader).expect("hello");
        assert_eq!(hello.packet_type, PacketType::Metadata);
        let capabilities = CapabilitySet {
            protocol_versions: vec![2],
            codecs: vec!["h264".into()],
            // The desktop will ask for 1080p60; the phone only offers this.
            video_modes: vec![VideoMode {
                width: 320,
                height: 180,
                fps: 30,
                min_bitrate_bps: 100_000,
                max_bitrate_bps: 50_000_000,
            }],
            audio_codecs: vec![],
        };
        session_packet(&SessionMessage::Capabilities(capabilities))
            .write_to(&mut stream)
            .unwrap();

        let start = read_packet(&mut reader).expect("start");
        let SessionMessage::Start(request) = serde_json::from_slice(&start.payload).unwrap() else {
            panic!("expected a start request");
        };
        let accepted = NegotiatedStream {
            session_id: SESSION,
            config: request.config,
            audio_enabled: false,
        };
        session_packet(&SessionMessage::StreamAccepted(accepted))
            .write_to(&mut stream)
            .unwrap();

        // Record whatever the desktop sends from now on.
        let reader_commands = Arc::clone(&thread_commands);
        thread::spawn(move || {
            while let Ok(packet) = read_packet(&mut reader) {
                if packet.packet_type == PacketType::ControlCmd {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&packet.payload)
                    {
                        if let Some(name) = value.get("cmd").and_then(|cmd| cmd.as_str()) {
                            reader_commands.lock().unwrap().push(name.to_string());
                        }
                    }
                }
            }
        });

        // Stream the clip.
        let clip = load_clip();
        MediaPacket::new(
            MediaKind::VideoConfig,
            MediaFlags::NONE,
            SESSION,
            1,
            MediaTimestamp(0),
            clip.config.clone(),
        )
        .write_to(&mut stream)
        .unwrap();

        for (index, frame) in clip.frames.iter().enumerate() {
            if script.hang_up_after == Some(index) {
                let _ = stream.shutdown(Shutdown::Both);
                thread_finished.store(true, Ordering::SeqCst);
                return;
            }
            let sequence = index as u64 + 1;
            if script.drop_video_sequence == Some(sequence) {
                continue;
            }
            let flags = if frame.key {
                MediaFlags::KEY_FRAME
            } else {
                MediaFlags::NONE
            };
            let sent = MediaPacket::new(
                MediaKind::Video,
                flags,
                SESSION,
                sequence,
                MediaTimestamp(index as u64 * 33_333_333),
                frame.data.clone(),
            )
            .write_to(&mut stream);
            if sent.is_err() {
                break;
            }
            let _ = stream.flush();
            thread::sleep(FRAME_INTERVAL);
        }
        thread_finished.store(true, Ordering::SeqCst);
        // Keep the connection open so the session stays alive for assertions.
        thread::sleep(Duration::from_secs(4));
    });

    Phone {
        port,
        commands,
        finished,
        handle: Some(handle),
    }
}

fn connect(phone: &Phone) -> CamdroidSession {
    init_ffmpeg();
    let config = SessionConfig {
        output: OutputMode::Standalone,
        audio: false,
        ..SessionConfig::default()
    };
    CamdroidSession::connect(
        ConnectionMode::Direct(format!("127.0.0.1:{}", phone.port)),
        config,
    )
    .expect("session connects")
}

// ---------------------------------------------------------------------------
// Observing the preview
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Tint {
    Red,
    Blue,
    Other,
}

/// Classifies the centre pixel of a preview frame.
fn tint(frame: &PreviewFrame) -> Tint {
    let offset = ((frame.height / 2) * frame.width + frame.width / 2) as usize * 3;
    let (r, g, b) = (
        i32::from(frame.rgb[offset]),
        i32::from(frame.rgb[offset + 1]),
        i32::from(frame.rgb[offset + 2]),
    );
    if r > b + 60 && r > g + 60 {
        Tint::Red
    } else if b > r + 60 && b > g + 60 {
        Tint::Blue
    } else {
        Tint::Other
    }
}

/// Follows the preview until `done` says so or the time runs out, returning
/// every distinct tint seen, in order.
fn watch_preview(session: &CamdroidSession, done: impl Fn(&[Tint]) -> bool) -> Vec<Tint> {
    let hub = session.preview();
    let mut seen: Vec<Tint> = Vec::new();
    let mut sequence = 0;
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline && !done(&seen) {
        if let PreviewWait::Frame(frame) = hub.wait_for_frame(sequence, Duration::from_millis(100))
        {
            sequence = frame.sequence;
            let current = tint(&frame);
            if seen.last() != Some(&current) {
                seen.push(current);
            }
        }
    }
    seen
}

fn ended_with_blue(seen: &[Tint]) -> bool {
    seen.last() == Some(&Tint::Blue)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn the_fixture_is_the_clip_these_tests_assume() {
    let clip = load_clip();
    assert_eq!(clip.frames.len(), 60);
    let keys: Vec<usize> = clip
        .frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame.key)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(keys, vec![0, 15, 30, 45]);
    assert!(!clip.config.is_empty());
}

#[test]
fn a_clean_stream_plays_from_red_to_blue_through_the_whole_pipeline() {
    let phone = spawn_phone(PhoneScript::CLEAN);
    let session = connect(&phone);

    let seen = watch_preview(&session, ended_with_blue);

    assert!(seen.contains(&Tint::Red), "never saw red: {seen:?}");
    assert!(ended_with_blue(&seen), "never reached blue: {seen:?}");
    let status = session.status();
    assert_eq!(status.telemetry.decode_errors, 0);
    assert!(status.connected);
    session.stop().unwrap();
}

#[test]
fn the_desktop_adapts_to_the_only_mode_the_phone_offers() {
    let phone = spawn_phone(PhoneScript::CLEAN);
    let session = connect(&phone);

    // The desktop asked for 1080p60; the phone only has 320x180@30.
    let stream = session.negotiated();
    assert_eq!(
        (stream.config.width, stream.config.height, stream.config.fps),
        (320, 180, 30)
    );
    let status = session.status();
    assert_eq!((status.width, status.height), (320, 180));
    session.stop().unwrap();
}

#[test]
fn a_lost_packet_is_never_decoded_over_and_the_stream_recovers_at_the_next_key_frame() {
    // Sequence 21 is frame 20, a P-frame in the middle of the red GOP.
    let phone = spawn_phone(PhoneScript {
        drop_video_sequence: Some(21),
        hang_up_after: None,
    });
    let session = connect(&phone);

    let seen = watch_preview(&session, ended_with_blue);
    while !phone.streamed_everything() {
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_millis(300));

    assert!(ended_with_blue(&seen), "never recovered to blue: {seen:?}");
    let telemetry = session.status().telemetry;
    // Frames 21..=29 reference the missing frame and must be held back.
    assert!(
        telemetry.frames_dropped >= 9,
        "expected the dependent frames to be dropped, got {}",
        telemetry.frames_dropped
    );
    assert_eq!(
        telemetry.decode_errors, 0,
        "nothing broken should reach the decoder"
    );
    assert!(
        phone.commands().iter().any(|cmd| cmd == "request_keyframe"),
        "the phone was never asked for a key frame: {:?}",
        phone.commands()
    );
    session.stop().unwrap();
}

#[test]
fn losing_the_phone_is_noticed_and_explained() {
    let phone = spawn_phone(PhoneScript {
        drop_video_sequence: None,
        hang_up_after: Some(5),
    });
    let session = connect(&phone);
    assert!(session.is_alive());

    let deadline = Instant::now() + WAIT;
    while session.is_alive() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        !session.is_alive(),
        "session never noticed the phone hanging up"
    );
    let reason = session.status().telemetry.disconnect_reason;
    assert!(
        reason
            .as_deref()
            .is_some_and(|text| text.contains("closed the connection")),
        "unexpected reason: {reason:?}"
    );
    session.stop().unwrap();
}

#[test]
fn stopping_tells_the_phone_and_releases_the_preview() {
    let phone = spawn_phone(PhoneScript::CLEAN);
    let session = connect(&phone);
    let hub = session.preview();
    let _ = watch_preview(&session, |seen| !seen.is_empty());

    session.stop().unwrap();

    assert!(!session.is_alive());
    let deadline = Instant::now() + Duration::from_secs(3);
    while !phone.commands().iter().any(|cmd| cmd == "stop") && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        phone.commands().iter().any(|cmd| cmd == "stop"),
        "{:?}",
        phone.commands()
    );
    assert!(
        matches!(
            hub.wait_for_frame(u64::MAX, Duration::from_millis(200)),
            PreviewWait::Closed
        ),
        "preview hub must be closed after stop"
    );
}

#[test]
fn connecting_to_nothing_fails_fast_with_a_clear_error() {
    init_ffmpeg();
    // Grab a free port, then close it so nothing is listening there.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let _probe = TcpStream::connect(("127.0.0.1", port)).err();

    let started = Instant::now();
    let result = CamdroidSession::connect(
        ConnectionMode::Direct(format!("127.0.0.1:{port}")),
        SessionConfig {
            output: OutputMode::Standalone,
            audio: false,
            ..SessionConfig::default()
        },
    );

    let error = match result {
        Ok(_) => panic!("connecting to a closed port must fail"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("Failed to connect"), "got: {error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}
