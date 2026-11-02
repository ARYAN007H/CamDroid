// TCP client for connecting to the CamDroid Android app.
//
// Handles both WiFi (direct IP) and USB (ADB-forwarded localhost) connections.
// Provides a reader thread that dispatches incoming v2.0 packets to low-latency
// bounded channels and automatically requests an IDR keyframe if congestion drops a frame.

use crate::protocol::{self, ControlCommand, Packet, PacketType};
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use log::{debug, error, info, warn};
use std::io::BufWriter;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Low-latency video frame queue depth (8 frames = ~133ms max ceiling at 60fps, down from 120).
const VIDEO_CHANNEL_CAPACITY: usize = 8;
const AUDIO_CHANNEL_CAPACITY: usize = 32;

/// Encoded media payload paired with its v2.0 header flags and hardware PTS.
#[derive(Debug, Clone)]
pub struct MediaPacket {
    pub flags: u8,
    pub pts_us: i64,
    pub data: Vec<u8>,
}

/// Channels that carry decoded packet payloads from the reader thread.
pub struct PacketChannels {
    pub video_rx: Receiver<MediaPacket>,
    pub audio_rx: Receiver<MediaPacket>,
    pub video_config_rx: Receiver<Vec<u8>>,
    pub audio_config_rx: Receiver<Vec<u8>>,
    pub metadata_rx: Receiver<Vec<u8>>,
}

/// Handle for sending packets back to the phone (control commands, heartbeats).
pub struct WriterHandle {
    tx: Sender<Packet>,
    _thread: thread::JoinHandle<()>,
}

impl WriterHandle {
    /// Queue a packet for sending to the phone. Non-blocking.
    pub fn send(&self, packet: Packet) -> Result<()> {
        self.tx
            .try_send(packet)
            .map_err(|_| anyhow::anyhow!("Writer channel full or closed"))
    }
}

/// Establish a TCP connection to the CamDroid phone server.
///
/// Returns the packet dispatch channels and a writer handle.
pub fn connect(
    addr: impl ToSocketAddrs + std::fmt::Debug + Clone,
    shutdown: Arc<AtomicBool>,
) -> Result<(PacketChannels, WriterHandle)> {
    let addr_debug = format!("{:?}", addr);
    info!("Connecting to {}...", addr_debug);

    let stream = TcpStream::connect_timeout(
        &addr
            .clone()
            .to_socket_addrs()
            .context("Invalid address")?
            .next()
            .context("No addresses resolved")?,
        Duration::from_secs(10),
    )
    .with_context(|| format!("Failed to connect to {}", addr_debug))?;

    stream
        .set_nodelay(true)
        .context("Failed to set TCP_NODELAY")?;

    info!("Connected to {}", addr_debug);

    let (video_tx, video_rx) = crossbeam_channel::bounded::<MediaPacket>(VIDEO_CHANNEL_CAPACITY);
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<MediaPacket>(AUDIO_CHANNEL_CAPACITY);
    let (video_config_tx, video_config_rx) = crossbeam_channel::bounded::<Vec<u8>>(8);
    let (audio_config_tx, audio_config_rx) = crossbeam_channel::bounded::<Vec<u8>>(8);
    let (metadata_tx, metadata_rx) = crossbeam_channel::bounded::<Vec<u8>>(32);

    let (writer_tx, writer_rx) = crossbeam_channel::bounded::<Packet>(64);
    let reader_writer_tx = writer_tx.clone();

    // Spawn the reader thread that continuously reads packets from TCP
    let reader_stream = stream
        .try_clone()
        .context("Failed to clone TCP stream for reader")?;
    let reader_shutdown = shutdown.clone();

    thread::Builder::new()
        .name("tcp-reader".into())
        .spawn(move || {
            let mut reader = std::io::BufReader::with_capacity(128 * 1024, reader_stream);
            let mut scratch = Vec::with_capacity(256 * 1024);
            let mut last_keyframe_req = Instant::now() - Duration::from_secs(5);

            loop {
                if reader_shutdown.load(Ordering::Relaxed) {
                    debug!("Reader thread: shutdown signal received");
                    break;
                }

                match protocol::read_packet_into(&mut reader, &mut scratch) {
                    Ok((packet_type, flags, pts_us)) => {
                        match packet_type {
                            PacketType::VideoFrame => {
                                let media_pkt = MediaPacket {
                                    flags,
                                    pts_us,
                                    data: scratch.clone(),
                                };
                                if let Err(crossbeam_channel::TrySendError::Full(_)) =
                                    video_tx.try_send(media_pkt)
                                {
                                    debug!("Video channel full, dropping frame & requesting IDR keyframe");
                                    if last_keyframe_req.elapsed() >= Duration::from_millis(300) {
                                        last_keyframe_req = Instant::now();
                                        if let Ok(kf_pkt) =
                                            Packet::control_cmd(&ControlCommand::RequestKeyframe)
                                        {
                                            let _ = reader_writer_tx.try_send(kf_pkt);
                                        }
                                    }
                                }
                            }
                            PacketType::AudioFrame => {
                                let media_pkt = MediaPacket {
                                    flags,
                                    pts_us,
                                    data: scratch.clone(),
                                };
                                let _ = audio_tx.try_send(media_pkt);
                            }
                            PacketType::VideoConfig => {
                                let _ = video_config_tx.try_send(scratch.clone());
                            }
                            PacketType::AudioConfig => {
                                let _ = audio_config_tx.try_send(scratch.clone());
                            }
                            PacketType::Metadata => {
                                let _ = metadata_tx.try_send(scratch.clone());
                            }
                            PacketType::Heartbeat => {
                                debug!("Heartbeat received (pts_us={})", pts_us);
                            }
                            PacketType::ControlCmd => {
                                warn!("Received unexpected ControlCmd from server");
                            }
                        }
                    }
                    Err(e) => {
                        if !reader_shutdown.load(Ordering::Relaxed) {
                            error!("Reader error: {:#}", e);
                            reader_shutdown.store(true, Ordering::Relaxed);
                        }
                        break;
                    }
                }
            }
            info!("Reader thread exiting");
        })
        .context("Failed to spawn reader thread")?;

    // Spawn the writer thread that sends queued packets to the phone.
    let writer_stream = stream
        .try_clone()
        .context("Failed to clone TCP stream for writer")?;
    let writer_shutdown = shutdown.clone();

    let writer_thread = thread::Builder::new()
        .name("tcp-writer".into())
        .spawn(move || {
            let mut writer = BufWriter::new(writer_stream);
            loop {
                match writer_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(packet) => {
                        if let Err(e) = protocol::write_packet(&mut writer, &packet) {
                            if !writer_shutdown.load(Ordering::Relaxed) {
                                error!("Writer error: {:#}", e);
                                writer_shutdown.store(true, Ordering::Relaxed);
                            }
                            break;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if writer_shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        if let Err(e) = protocol::write_packet(&mut writer, &Packet::heartbeat()) {
                            if !writer_shutdown.load(Ordering::Relaxed) {
                                error!("Heartbeat write error: {:#}", e);
                                writer_shutdown.store(true, Ordering::Relaxed);
                            }
                            break;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        break;
                    }
                }
            }
            info!("Writer thread exiting");
        })
        .context("Failed to spawn writer thread")?;

    let channels = PacketChannels {
        video_rx,
        audio_rx,
        video_config_rx,
        audio_config_rx,
        metadata_rx,
    };

    let writer_handle = WriterHandle {
        tx: writer_tx,
        _thread: writer_thread,
    };

    Ok((channels, writer_handle))
}

