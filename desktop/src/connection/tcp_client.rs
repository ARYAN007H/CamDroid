//! TCP client for the CamDroid Android app.
//!
//! Handles WiFi (direct IP) and USB (ADB-forwarded localhost) connections.
//! A reader thread parses incoming packets and routes them to typed channels;
//! a writer thread sends queued control commands and keep-alive heartbeats.
//!
//! The reader is also the first line of defence for video integrity: it runs
//! every video unit through a [`VideoGate`] so that a lost or discarded
//! packet never reaches the decoder as a silently broken reference chain.

use crate::media_queue::{GateDecision, TimedMediaFrame, VideoGate};
use crate::protocol::{
    self,
    v2::{MediaFlags, MediaKind, MediaPacket, MediaTimestamp, SessionId},
    ControlCommand, Packet, PacketType,
};
use crate::telemetry::Telemetry;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use log::{debug, error, info, warn};
use std::io::{self, BufWriter, Read};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The protocol sends heartbeats every 2 s; ten seconds of silence means the
/// link is dead even if the OS never reports it.
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(10);
const WRITER_IDLE_HEARTBEAT: Duration = Duration::from_secs(2);
/// Encoded video units the decoder may fall behind by before we resync.
const VIDEO_BACKLOG: usize = 8;
/// About 1.1 s of AAC; the audio decoder additionally trims by timestamp.
const AUDIO_BACKLOG: usize = 48;
/// Minimum spacing between key frame requests, so a stuck stream does not
/// flood the phone's encoder.
const KEYFRAME_REQUEST_SPACING: Duration = Duration::from_millis(500);

/// Channels that carry decoded packet payloads from the reader thread.
pub struct PacketChannels {
    pub video_rx: Receiver<TimedMediaFrame>,
    pub audio_rx: Receiver<TimedMediaFrame>,
    pub video_config_rx: Receiver<Vec<u8>>,
    pub audio_config_rx: Receiver<Vec<u8>>,
    pub metadata_rx: Receiver<Vec<u8>>,
}

/// Rate-limited way for any pipeline stage to ask the phone for a key frame.
#[derive(Clone)]
pub struct KeyframeRequester {
    tx: Sender<Packet>,
    last_request: Arc<Mutex<Option<Instant>>>,
}

impl KeyframeRequester {
    fn new(tx: Sender<Packet>) -> Self {
        Self {
            tx,
            last_request: Arc::new(Mutex::new(None)),
        }
    }

    /// Asks the phone's encoder for an immediate key frame. Calls closer
    /// together than the rate limit are ignored.
    pub fn request(&self) {
        let now = Instant::now();
        {
            let mut last = self
                .last_request
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if last.is_some_and(|at| now.duration_since(at) < KEYFRAME_REQUEST_SPACING) {
                return;
            }
            *last = Some(now);
        }
        if let Ok(packet) = Packet::control_cmd(&ControlCommand::RequestKeyframe) {
            // Non-blocking on purpose: a full writer queue means the phone is
            // already being flooded and a repeat request would not help.
            let _ = self.tx.try_send(packet);
        }
    }
}

/// Handle for sending packets back to the phone (control commands, heartbeats).
pub struct WriterHandle {
    tx: Sender<Packet>,
    socket: TcpStream,
    _thread: thread::JoinHandle<()>,
}

impl WriterHandle {
    /// Queue a packet for sending to the phone. Non-blocking.
    pub fn send(&self, packet: Packet) -> Result<()> {
        self.tx
            .send(packet)
            .map_err(|_| anyhow::anyhow!("Writer channel closed"))
    }

    /// Waits until queued packets have left the queue (or `timeout` passes),
    /// giving a final command such as `stop` a chance to reach the phone.
    pub fn drain(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.tx.is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        // The packet may have been dequeued but not yet flushed to the socket.
        thread::sleep(Duration::from_millis(20));
    }

    /// Closes the socket in both directions, which also wakes the reader
    /// thread out of its blocking read.
    pub fn close(&self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

impl Drop for WriterHandle {
    fn drop(&mut self) {
        self.close();
    }
}

/// Everything produced by [`connect`].
pub struct Connection {
    pub channels: PacketChannels,
    pub writer: WriterHandle,
    pub keyframes: KeyframeRequester,
    /// Set to true for codecs whose frames are all self-contained (MJPEG), so
    /// the video gate does not wait for key frame flags that never come.
    pub intra_only: Arc<AtomicBool>,
}

/// Establish a TCP connection to the CamDroid phone server.
///
/// On any unexpected loss of the link the `shutdown` flag is raised and the
/// reason is recorded in `telemetry`, so callers learn about it by polling.
pub fn connect(addr: &str, shutdown: Arc<AtomicBool>, telemetry: Telemetry) -> Result<Connection> {
    info!("Connecting to {addr}...");
    let stream = open_stream(addr)?;
    info!("Connected to {addr}");

    let (video_tx, video_rx) = crossbeam_channel::bounded::<TimedMediaFrame>(VIDEO_BACKLOG);
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<TimedMediaFrame>(AUDIO_BACKLOG);
    let (video_config_tx, video_config_rx) = crossbeam_channel::bounded::<Vec<u8>>(4);
    let (audio_config_tx, audio_config_rx) = crossbeam_channel::bounded::<Vec<u8>>(4);
    let (metadata_tx, metadata_rx) = crossbeam_channel::bounded::<Vec<u8>>(16);
    let (writer_tx, writer_rx) = crossbeam_channel::bounded::<Packet>(64);

    let keyframes = KeyframeRequester::new(writer_tx.clone());
    let intra_only = Arc::new(AtomicBool::new(false));

    let dispatcher = Dispatcher {
        video_drain_rx: video_rx.clone(),
        video_tx,
        audio_drain_rx: audio_rx.clone(),
        audio_tx,
        video_config_tx,
        audio_config_tx,
        metadata_tx,
        gate: VideoGate::new(),
        keyframes: keyframes.clone(),
        telemetry: telemetry.clone(),
        intra_only: Arc::clone(&intra_only),
        legacy_video_sequence: 0,
        legacy_audio_sequence: 0,
    };

    let reader_stream = stream
        .try_clone()
        .context("Failed to clone TCP stream for reader")?;
    let reader_shutdown = Arc::clone(&shutdown);
    let reader_telemetry = telemetry.clone();
    thread::Builder::new()
        .name("tcp-reader".into())
        .spawn(move || {
            reader_loop(
                reader_stream,
                dispatcher,
                &reader_shutdown,
                &reader_telemetry,
            );
            info!("Reader thread exiting");
        })
        .context("Failed to spawn reader thread")?;

    let writer_stream = stream
        .try_clone()
        .context("Failed to clone TCP stream for writer")?;
    let writer_thread = thread::Builder::new()
        .name("tcp-writer".into())
        .spawn(move || {
            writer_loop(writer_stream, &writer_rx, &shutdown, &telemetry);
            info!("Writer thread exiting");
        })
        .context("Failed to spawn writer thread")?;

    Ok(Connection {
        channels: PacketChannels {
            video_rx,
            audio_rx,
            video_config_rx,
            audio_config_rx,
            metadata_rx,
        },
        writer: WriterHandle {
            tx: writer_tx,
            socket: stream,
            _thread: writer_thread,
        },
        keyframes,
        intra_only,
    })
}

fn open_stream(addr: &str) -> Result<TcpStream> {
    let candidates: Vec<_> = addr
        .to_socket_addrs()
        .with_context(|| format!("Invalid address '{addr}'"))?
        .collect();
    let mut last_error = None;
    for candidate in &candidates {
        match TcpStream::connect_timeout(candidate, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_nodelay(true)
                    .context("Failed to set TCP_NODELAY")?;
                stream
                    .set_read_timeout(Some(LIVENESS_TIMEOUT))
                    .context("Failed to set read timeout")?;
                return Ok(stream);
            }
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) => Err(error).with_context(|| format!("Failed to connect to {addr}")),
        None => anyhow::bail!("No addresses resolved for '{addr}'"),
    }
}

enum ReceivedPacket {
    V1(Packet),
    V2(MediaPacket),
}

/// Reads one packet in either wire format. v2 packets begin with the magic
/// `CMD2`; anything else is a legacy v1 type byte.
fn read_any_packet(reader: &mut impl Read) -> io::Result<ReceivedPacket> {
    let mut first = [0_u8; 1];
    reader.read_exact(&mut first)?;
    if first[0] != b'C' {
        return protocol::wire::read_packet_after_type(reader, first[0])
            .map(ReceivedPacket::V1)
            .map_err(io::Error::other);
    }
    let mut rest = [0_u8; 3];
    reader.read_exact(&mut rest)?;
    if rest != *b"MD2" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid protocol magic",
        ));
    }
    MediaPacket::read_after_magic(reader)
        .map(ReceivedPacket::V2)
        .map_err(io::Error::other)
}

fn describe_disconnect(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset => {
            "The phone closed the connection.".to_string()
        }
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => format!(
            "The phone stopped responding for {} seconds.",
            LIVENESS_TIMEOUT.as_secs()
        ),
        _ => format!("Lost connection to the phone: {error}"),
    }
}

fn reader_loop(
    stream: TcpStream,
    mut dispatcher: Dispatcher,
    shutdown: &AtomicBool,
    telemetry: &Telemetry,
) {
    let mut reader = io::BufReader::new(stream);
    while !shutdown.load(Ordering::Relaxed) {
        match read_any_packet(&mut reader) {
            Ok(packet) => dispatcher.dispatch(packet),
            Err(error) => {
                if !shutdown.load(Ordering::Relaxed) {
                    error!("Reader error: {error}");
                    telemetry.set_disconnect_reason(describe_disconnect(&error));
                    shutdown.store(true, Ordering::Relaxed);
                }
                break;
            }
        }
    }
}

fn writer_loop(
    stream: TcpStream,
    packets: &Receiver<Packet>,
    shutdown: &AtomicBool,
    telemetry: &Telemetry,
) {
    let mut writer = BufWriter::new(stream);
    while !shutdown.load(Ordering::Relaxed) {
        let outcome = match packets.recv_timeout(WRITER_IDLE_HEARTBEAT) {
            Ok(packet) => protocol::write_packet(&mut writer, &packet),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                protocol::write_packet(&mut writer, &Packet::heartbeat())
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        if let Err(error) = outcome {
            if !shutdown.load(Ordering::Relaxed) {
                error!("Writer error: {error:#}");
                telemetry.set_disconnect_reason("Could not send data to the phone.");
                shutdown.store(true, Ordering::Relaxed);
            }
            break;
        }
    }
}

/// Phone-originated status update (`{"type":"status","battery":72,...}`).
#[derive(Debug, PartialEq, Eq)]
struct StatusReport {
    battery_percent: Option<u8>,
}

fn parse_status(payload: &[u8]) -> Option<StatusReport> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    if value.get("type")?.as_str()? != "status" {
        return None;
    }
    let battery_percent = value
        .get("battery")
        .and_then(serde_json::Value::as_u64)
        .map(|percent| percent.min(100) as u8);
    Some(StatusReport { battery_percent })
}

/// Routes parsed packets to their consumers and enforces video admission.
struct Dispatcher {
    video_tx: Sender<TimedMediaFrame>,
    video_drain_rx: Receiver<TimedMediaFrame>,
    audio_tx: Sender<TimedMediaFrame>,
    audio_drain_rx: Receiver<TimedMediaFrame>,
    video_config_tx: Sender<Vec<u8>>,
    audio_config_tx: Sender<Vec<u8>>,
    metadata_tx: Sender<Vec<u8>>,
    gate: VideoGate,
    keyframes: KeyframeRequester,
    telemetry: Telemetry,
    intra_only: Arc<AtomicBool>,
    legacy_video_sequence: u64,
    legacy_audio_sequence: u64,
}

impl Dispatcher {
    fn dispatch(&mut self, packet: ReceivedPacket) {
        match packet {
            ReceivedPacket::V1(packet) => self.dispatch_v1(packet),
            ReceivedPacket::V2(packet) => self.dispatch_v2(packet),
        }
    }

    fn dispatch_v1(&mut self, packet: Packet) {
        match packet.packet_type {
            PacketType::VideoFrame => {
                self.legacy_video_sequence = self.legacy_video_sequence.wrapping_add(1);
                let frame =
                    legacy_frame(MediaKind::Video, self.legacy_video_sequence, packet.payload);
                self.enqueue_video(frame);
            }
            PacketType::AudioFrame => {
                self.legacy_audio_sequence = self.legacy_audio_sequence.wrapping_add(1);
                let frame =
                    legacy_frame(MediaKind::Audio, self.legacy_audio_sequence, packet.payload);
                self.enqueue_audio(frame);
            }
            PacketType::VideoConfig => {
                let _ = self.video_config_tx.try_send(packet.payload);
            }
            PacketType::AudioConfig => {
                let _ = self.audio_config_tx.try_send(packet.payload);
            }
            PacketType::Metadata => self.dispatch_metadata(packet.payload),
            PacketType::Heartbeat => debug!("Heartbeat received"),
            PacketType::ControlCmd => warn!("Received unexpected ControlCmd from server"),
        }
    }

    fn dispatch_v2(&mut self, packet: MediaPacket) {
        match packet.kind {
            MediaKind::Video => self.enqueue_video(TimedMediaFrame::from_packet(packet)),
            MediaKind::Audio => self.enqueue_audio(TimedMediaFrame::from_packet(packet)),
            MediaKind::VideoConfig => {
                let _ = self.video_config_tx.try_send(packet.payload);
            }
            MediaKind::AudioConfig => {
                let _ = self.audio_config_tx.try_send(packet.payload);
            }
            MediaKind::Metadata => self.dispatch_metadata(packet.payload),
            MediaKind::Control | MediaKind::Heartbeat => {}
        }
    }

    fn dispatch_metadata(&self, payload: Vec<u8>) {
        match parse_status(&payload) {
            Some(StatusReport { battery_percent }) => {
                if let Some(percent) = battery_percent {
                    self.telemetry.set_battery_percent(percent);
                }
            }
            None => {
                let _ = self.metadata_tx.try_send(payload);
            }
        }
    }

    /// Audio frames are independent, so on overflow the oldest is evicted.
    fn enqueue_audio(&self, frame: TimedMediaFrame) {
        if let Err(TrySendError::Full(frame)) = self.audio_tx.try_send(frame) {
            let _ = self.audio_drain_rx.try_recv();
            let _ = self.audio_tx.try_send(frame);
        }
    }

    /// Video admission. Frames only reach the decoder if the gate says its
    /// reference chain is intact; on overflow everything queued is discarded
    /// and the stream restarts from the next key frame.
    fn enqueue_video(&mut self, frame: TimedMediaFrame) {
        let intra_only = self.intra_only.load(Ordering::Relaxed);
        if intra_only {
            if let Err(TrySendError::Full(frame)) = self.video_tx.try_send(frame) {
                let _ = self.video_drain_rx.try_recv();
                let _ = self.video_tx.try_send(frame);
                self.telemetry.record_dropped(1);
            }
            return;
        }

        if self.gate.observe(&frame) == GateDecision::Drop {
            self.telemetry.record_dropped(1);
            self.keyframes.request();
            return;
        }

        if let Err(TrySendError::Full(frame)) = self.video_tx.try_send(frame) {
            let discarded = self.drain_video_backlog();
            self.telemetry.record_dropped(discarded);
            self.gate.request_resync();
            // A key frame arriving right now can restart the stream at once.
            if self.gate.observe(&frame) == GateDecision::Admit {
                let _ = self.video_tx.try_send(frame);
            } else {
                self.telemetry.record_dropped(1);
                self.keyframes.request();
            }
        }
    }

    fn drain_video_backlog(&self) -> u64 {
        self.video_drain_rx.try_iter().count() as u64
    }
}

fn legacy_frame(kind: MediaKind, sequence: u64, payload: Vec<u8>) -> TimedMediaFrame {
    TimedMediaFrame::from_packet(MediaPacket::new(
        kind,
        MediaFlags::NONE,
        SessionId(0),
        sequence,
        MediaTimestamp(0),
        payload,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Harness {
        dispatcher: Dispatcher,
        video_rx: Receiver<TimedMediaFrame>,
        audio_rx: Receiver<TimedMediaFrame>,
        metadata_rx: Receiver<Vec<u8>>,
        writer_rx: Receiver<Packet>,
        telemetry: Telemetry,
    }

    fn harness(video_capacity: usize) -> Harness {
        let (video_tx, video_rx) = crossbeam_channel::bounded(video_capacity);
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(2);
        let (video_config_tx, _video_config_rx) = crossbeam_channel::bounded(4);
        let (audio_config_tx, _audio_config_rx) = crossbeam_channel::bounded(4);
        let (metadata_tx, metadata_rx) = crossbeam_channel::bounded(4);
        let (writer_tx, writer_rx) = crossbeam_channel::bounded(8);
        let telemetry = Telemetry::default();
        let dispatcher = Dispatcher {
            video_drain_rx: video_rx.clone(),
            video_tx,
            audio_drain_rx: audio_rx.clone(),
            audio_tx,
            video_config_tx,
            audio_config_tx,
            metadata_tx,
            gate: VideoGate::new(),
            keyframes: KeyframeRequester::new(writer_tx),
            telemetry: telemetry.clone(),
            intra_only: Arc::new(AtomicBool::new(false)),
            legacy_video_sequence: 0,
            legacy_audio_sequence: 0,
        };
        Harness {
            dispatcher,
            video_rx,
            audio_rx,
            metadata_rx,
            writer_rx,
            telemetry,
        }
    }

    fn video(sequence: u64, flags: MediaFlags) -> MediaPacket {
        MediaPacket::new(
            MediaKind::Video,
            flags,
            SessionId(1),
            sequence,
            MediaTimestamp(sequence * 16_000_000),
            vec![sequence as u8],
        )
    }

    fn sequences(rx: &Receiver<TimedMediaFrame>) -> Vec<u64> {
        rx.try_iter().map(|frame| frame.packet.sequence).collect()
    }

    #[test]
    fn video_is_held_back_until_the_first_key_frame() {
        let mut h = harness(8);
        h.dispatcher.dispatch_v2(video(1, MediaFlags::NONE));
        h.dispatcher.dispatch_v2(video(2, MediaFlags::KEY_FRAME));
        h.dispatcher.dispatch_v2(video(3, MediaFlags::NONE));

        assert_eq!(sequences(&h.video_rx), vec![2, 3]);
    }

    #[test]
    fn overflow_discards_the_backlog_instead_of_evicting_a_single_frame() {
        let mut h = harness(3);
        h.dispatcher.dispatch_v2(video(1, MediaFlags::KEY_FRAME));
        h.dispatcher.dispatch_v2(video(2, MediaFlags::NONE));
        h.dispatcher.dispatch_v2(video(3, MediaFlags::NONE));
        // Queue is full; frame 4 would previously have evicted frame 1 and
        // left 2, 3, 4 referencing a missing key frame.
        h.dispatcher.dispatch_v2(video(4, MediaFlags::NONE));

        assert_eq!(sequences(&h.video_rx), Vec::<u64>::new());
        assert_eq!(h.telemetry.snapshot().frames_dropped, 4);
    }

    #[test]
    fn after_an_overflow_only_a_key_frame_restarts_the_stream() {
        let mut h = harness(2);
        h.dispatcher.dispatch_v2(video(1, MediaFlags::KEY_FRAME));
        h.dispatcher.dispatch_v2(video(2, MediaFlags::NONE));
        h.dispatcher.dispatch_v2(video(3, MediaFlags::NONE)); // overflow
        let _ = sequences(&h.video_rx);

        h.dispatcher.dispatch_v2(video(4, MediaFlags::NONE));
        h.dispatcher.dispatch_v2(video(5, MediaFlags::KEY_FRAME));
        h.dispatcher.dispatch_v2(video(6, MediaFlags::NONE));

        assert_eq!(sequences(&h.video_rx), vec![5, 6]);
    }

    #[test]
    fn a_key_frame_that_overflows_the_queue_restarts_the_stream_immediately() {
        let mut h = harness(2);
        h.dispatcher.dispatch_v2(video(1, MediaFlags::KEY_FRAME));
        h.dispatcher.dispatch_v2(video(2, MediaFlags::NONE));
        h.dispatcher.dispatch_v2(video(3, MediaFlags::KEY_FRAME));

        assert_eq!(sequences(&h.video_rx), vec![3]);
    }

    #[test]
    fn dropping_video_asks_the_phone_for_a_key_frame_once() {
        let mut h = harness(8);
        for sequence in 1..=5 {
            h.dispatcher.dispatch_v2(video(sequence, MediaFlags::NONE));
        }

        let requests: Vec<_> = h.writer_rx.try_iter().collect();
        assert_eq!(requests.len(), 1, "requests must be rate limited");
        assert_eq!(requests[0].packet_type, PacketType::ControlCmd);
    }

    #[test]
    fn intra_only_streams_bypass_the_gate_and_keep_the_newest_frames() {
        let mut h = harness(2);
        h.dispatcher.intra_only.store(true, Ordering::Relaxed);
        for sequence in 1..=3 {
            h.dispatcher.dispatch_v2(video(sequence, MediaFlags::NONE));
        }

        assert_eq!(sequences(&h.video_rx), vec![2, 3]);
    }

    #[test]
    fn legacy_v1_video_is_admitted_without_flags() {
        let mut h = harness(8);
        h.dispatcher
            .dispatch_v1(Packet::new(PacketType::VideoFrame, vec![1, 2, 3]));

        assert_eq!(sequences(&h.video_rx), vec![1]);
    }

    #[test]
    fn audio_overflow_evicts_the_oldest_frame() {
        let mut h = harness(8);
        for sequence in 1..=3 {
            h.dispatcher.dispatch_v2(MediaPacket::new(
                MediaKind::Audio,
                MediaFlags::NONE,
                SessionId(1),
                sequence,
                MediaTimestamp(sequence),
                vec![],
            ));
        }

        assert_eq!(sequences(&h.audio_rx), vec![2, 3]);
    }

    #[test]
    fn status_updates_feed_telemetry_and_are_not_forwarded_as_handshake_data() {
        let mut h = harness(8);
        h.dispatcher.dispatch_v1(Packet::new(
            PacketType::Metadata,
            br#"{"type":"status","battery":64}"#.to_vec(),
        ));
        h.dispatcher.dispatch_v1(Packet::new(
            PacketType::Metadata,
            br#"{"type":"capabilities","data":{}}"#.to_vec(),
        ));

        assert_eq!(h.telemetry.snapshot().battery_percent, Some(64));
        assert_eq!(h.metadata_rx.try_iter().count(), 1);
    }

    #[test]
    fn status_parsing_ignores_other_message_types_and_garbage() {
        assert_eq!(parse_status(b"not json"), None);
        assert_eq!(parse_status(br#"{"type":"capabilities"}"#), None);
        assert_eq!(parse_status(br#"{"battery":50}"#), None);
        assert_eq!(
            parse_status(br#"{"type":"status","battery":900}"#),
            Some(StatusReport {
                battery_percent: Some(100)
            })
        );
        assert_eq!(
            parse_status(br#"{"type":"status"}"#),
            Some(StatusReport {
                battery_percent: None
            })
        );
    }

    #[test]
    fn disconnect_reasons_are_written_for_humans() {
        let eof = io::Error::from(io::ErrorKind::UnexpectedEof);
        assert!(describe_disconnect(&eof).contains("closed the connection"));
        let timeout = io::Error::from(io::ErrorKind::WouldBlock);
        assert!(describe_disconnect(&timeout).contains("stopped responding"));
    }
}
