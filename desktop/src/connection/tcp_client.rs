// TCP client for connecting to the CamDroid Android app.
//
// Handles both WiFi (direct IP) and USB (ADB-forwarded localhost) connections.
// Provides a reader thread that dispatches incoming packets to typed channels,
// and a writer handle for sending outbound control commands.

use crate::media_queue::TimedMediaFrame;
use crate::protocol::{
    self,
    v2::{MediaFlags, MediaKind, MediaPacket, MediaTimestamp, SessionId},
    Packet, PacketType,
};
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use log::{debug, error, info, warn};
use std::io::BufWriter;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Channels that carry decoded packet payloads from the reader thread.
pub struct PacketChannels {
    pub video_rx: Receiver<TimedMediaFrame>,
    pub audio_rx: Receiver<TimedMediaFrame>,
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
            .send(packet)
            .map_err(|_| anyhow::anyhow!("Writer channel closed"))
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

    // Keep only a tiny video backlog: stale video is worse than a dropped frame.
    let (video_tx, video_rx) = crossbeam_channel::bounded::<TimedMediaFrame>(3);
    let video_evict_rx = video_rx.clone();
    // 48 AAC frames is an approximately 1.1-second emergency cap; the decoder
    // additionally trims by sender timestamps before decoding queued audio.
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<TimedMediaFrame>(48);
    let audio_evict_rx = audio_rx.clone();
    let (video_config_tx, video_config_rx) = crossbeam_channel::bounded::<Vec<u8>>(4);
    let (audio_config_tx, audio_config_rx) = crossbeam_channel::bounded::<Vec<u8>>(4);
    let (metadata_tx, metadata_rx) = crossbeam_channel::bounded::<Vec<u8>>(16);

    // Spawn the reader thread that continuously reads packets from TCP
    // and dispatches them to the appropriate channel.
    let reader_stream = stream
        .try_clone()
        .context("Failed to clone TCP stream for reader")?;
    let reader_shutdown = shutdown.clone();

    thread::Builder::new()
        .name("tcp-reader".into())
        .spawn(move || {
            let mut reader = std::io::BufReader::new(reader_stream);
            let mut legacy_video_sequence = 0_u64;
            let mut legacy_audio_sequence = 0_u64;
            loop {
                if reader_shutdown.load(Ordering::Relaxed) {
                    debug!("Reader thread: shutdown signal received");
                    break;
                }

                let mut first_byte = [0_u8; 1];
                let received = match reader.read_exact(&mut first_byte) {
                    Ok(()) if first_byte[0] == b'C' => {
                        let mut rest_magic = [0_u8; 3];
                        reader.read_exact(&mut rest_magic).and_then(|_| {
                            if rest_magic == *b"MD2" {
                                protocol::v2::MediaPacket::read_after_magic(&mut reader)
                                    .map(ReceivedPacket::V2)
                                    .map_err(std::io::Error::other)
                            } else {
                                Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "invalid protocol magic",
                                ))
                            }
                        })
                    }
                    Ok(()) => protocol::wire::read_packet_after_type(&mut reader, first_byte[0])
                        .map(ReceivedPacket::V1)
                        .map_err(std::io::Error::other),
                    Err(error) => Err(error),
                };
                match received {
                    Ok(ReceivedPacket::V1(packet)) => {
                        let result = match packet.packet_type {
                            PacketType::VideoFrame => {
                                legacy_video_sequence = legacy_video_sequence.wrapping_add(1);
                                send_latest(
                                    &video_tx,
                                    &video_evict_rx,
                                    TimedMediaFrame::from_packet(MediaPacket::new(
                                        MediaKind::Video,
                                        MediaFlags::NONE,
                                        SessionId(0),
                                        legacy_video_sequence,
                                        MediaTimestamp(0),
                                        packet.payload,
                                    )),
                                );
                                continue;
                            }
                            PacketType::AudioFrame => {
                                legacy_audio_sequence = legacy_audio_sequence.wrapping_add(1);
                                send_latest(
                                    &audio_tx,
                                    &audio_evict_rx,
                                    TimedMediaFrame::from_packet(MediaPacket::new(
                                        MediaKind::Audio,
                                        MediaFlags::NONE,
                                        SessionId(0),
                                        legacy_audio_sequence,
                                        MediaTimestamp(0),
                                        packet.payload,
                                    )),
                                );
                                continue;
                            }
                            PacketType::VideoConfig => video_config_tx.try_send(packet.payload),
                            PacketType::AudioConfig => audio_config_tx.try_send(packet.payload),
                            PacketType::Metadata => metadata_tx.try_send(packet.payload),
                            PacketType::Heartbeat => {
                                debug!("Heartbeat received");
                                continue;
                            }
                            PacketType::ControlCmd => {
                                // Server shouldn't send control commands, but handle gracefully
                                warn!("Received unexpected ControlCmd from server");
                                continue;
                            }
                        };

                        if let Err(crossbeam_channel::TrySendError::Full(_)) = result {
                            // Drop the frame rather than blocking — this is expected
                            // under load and the consumer will catch up.
                            debug!("Channel full for {:?}, dropping packet", packet.packet_type);
                        }
                    }
                    Ok(ReceivedPacket::V2(packet)) => match packet.kind {
                        MediaKind::Video => send_latest(
                            &video_tx,
                            &video_evict_rx,
                            TimedMediaFrame::from_packet(packet),
                        ),
                        MediaKind::Audio => send_latest(
                            &audio_tx,
                            &audio_evict_rx,
                            TimedMediaFrame::from_packet(packet),
                        ),
                        MediaKind::VideoConfig => {
                            let _ = video_config_tx.try_send(packet.payload.clone());
                        }
                        MediaKind::AudioConfig => {
                            let _ = audio_config_tx.try_send(packet.payload.clone());
                        }
                        MediaKind::Metadata => {
                            let _ = metadata_tx.try_send(packet.payload.clone());
                        }
                        MediaKind::Control | MediaKind::Heartbeat => {}
                    },
                    Err(e) => {
                        if reader_shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        error!("Reader error: {:#}", e);
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
    let (writer_tx, writer_rx) = crossbeam_channel::bounded::<Packet>(64);
    let writer_shutdown = shutdown.clone();

    let writer_thread = thread::Builder::new()
        .name("tcp-writer".into())
        .spawn(move || {
            let mut writer = BufWriter::new(writer_stream);
            loop {
                // Use a timeout so we can check the shutdown flag periodically.
                match writer_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(packet) => {
                        if let Err(e) = protocol::write_packet(&mut writer, &packet) {
                            if writer_shutdown.load(Ordering::Relaxed) {
                                break;
                            }
                            error!("Writer error: {:#}", e);
                            break;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if writer_shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        // Send a heartbeat to keep the connection alive
                        if let Err(e) = protocol::write_packet(&mut writer, &Packet::heartbeat()) {
                            if writer_shutdown.load(Ordering::Relaxed) {
                                break;
                            }
                            error!("Heartbeat write error: {:#}", e);
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

enum ReceivedPacket {
    V1(Packet),
    V2(MediaPacket),
}

/// Enqueue a video payload without allowing stale frames to build up. The
/// cloned receiver is only used by the reader thread to evict one oldest item
/// when the consumer falls behind.
fn send_latest<T>(sender: &Sender<T>, eviction_rx: &Receiver<T>, item: T) {
    match sender.try_send(item) {
        Ok(()) => {}
        Err(crossbeam_channel::TrySendError::Full(item)) => {
            let _ = eviction_rx.try_recv();
            if sender.try_send(item).is_err() {
                debug!("video channel remained full after stale-frame eviction");
            }
        }
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
            debug!("video consumer disconnected");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::send_latest;
    use crossbeam_channel::bounded;

    #[test]
    fn full_video_channel_discards_oldest_and_retains_newest() {
        let (tx, rx) = bounded(3);
        let eviction_rx = rx.clone();
        for value in 1..=4 {
            send_latest(&tx, &eviction_rx, value);
        }

        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![2, 3, 4]);
    }
}
