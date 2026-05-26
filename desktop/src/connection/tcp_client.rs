// TCP client for connecting to the CamDroid Android app.
//
// Handles both WiFi (direct IP) and USB (ADB-forwarded localhost) connections.
// Provides a reader thread that dispatches incoming packets to typed channels,
// and a writer handle for sending outbound control commands.

use crate::protocol::{self, Packet, PacketType};
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use log::{debug, error, info, warn};
use std::io::BufWriter;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Channels that carry decoded packet payloads from the reader thread.
pub struct PacketChannels {
    pub video_rx: Receiver<Vec<u8>>,
    pub audio_rx: Receiver<Vec<u8>>,
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

    // Create dispatch channels with bounded capacity to apply backpressure.
    // Video frames get more buffer since they arrive at 30-60 fps.
    let (video_tx, video_rx) = crossbeam_channel::bounded::<Vec<u8>>(120);
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<Vec<u8>>(240);
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
            loop {
                if reader_shutdown.load(Ordering::Relaxed) {
                    debug!("Reader thread: shutdown signal received");
                    break;
                }

                match protocol::read_packet(&mut reader) {
                    Ok(packet) => {
                        let result = match packet.packet_type {
                            PacketType::VideoFrame => video_tx.try_send(packet.payload),
                            PacketType::AudioFrame => audio_tx.try_send(packet.payload),
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
                            debug!(
                                "Channel full for {:?}, dropping packet",
                                packet.packet_type
                            );
                        }
                    }
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
                        if let Err(e) =
                            protocol::write_packet(&mut writer, &Packet::heartbeat())
                        {
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
