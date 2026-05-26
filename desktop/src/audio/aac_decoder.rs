// AAC audio decoder using FFmpeg.
//
// Receives AAC frames (with ADTS headers) from the Android app,
// decodes them to raw PCM S16LE samples, and sends them to the
// PulseAudio output for playback as a virtual microphone.

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

extern crate ffmpeg_next as ffmpeg;

use ffmpeg::codec;
use ffmpeg::software::resampling;
use ffmpeg::util::format::sample::Sample;
use ffmpeg::util::channel_layout::ChannelLayout;

/// Spawn a thread that decodes AAC audio and outputs PCM S16LE samples.
///
/// # Arguments
/// * `audio_config_rx` - Channel receiving AudioSpecificConfig data
/// * `audio_rx` - Channel receiving AAC frames (with ADTS headers)
/// * `pcm_tx` - Channel to send decoded PCM S16LE data
/// * `shutdown` - Shutdown signal
pub fn spawn_audio_decoder_thread(
    audio_config_rx: Receiver<Vec<u8>>,
    audio_rx: Receiver<Vec<u8>>,
    pcm_tx: Sender<Vec<u8>>,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::Builder::new()
        .name("audio-decoder".into())
        .spawn(move || {
            if let Err(e) = audio_decoder_loop(audio_config_rx, audio_rx, pcm_tx, shutdown) {
                error!("Audio decoder error: {:#}", e);
            }
            info!("Audio decoder thread exiting");
        })
        .context("Failed to spawn audio decoder thread")?;

    Ok(handle)
}

/// Main audio decoder loop.
fn audio_decoder_loop(
    audio_config_rx: Receiver<Vec<u8>>,
    audio_rx: Receiver<Vec<u8>>,
    pcm_tx: Sender<Vec<u8>>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    info!("Audio decoder starting");

    // Find the AAC decoder
    let codec = codec::decoder::find(codec::Id::AAC)
        .context("FFmpeg AAC decoder not found")?;

    let mut context = codec::Context::new_with_codec(codec);

    // Optionally set extradata from AudioSpecificConfig if received
    match audio_config_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(config_data) => {
            info!(
                "Received audio config data: {} bytes",
                config_data.len()
            );
            unsafe {
                let extradata_ptr =
                    ffmpeg::ffi::av_mallocz(config_data.len() + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize)
                        as *mut u8;
                if !extradata_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(
                        config_data.as_ptr(),
                        extradata_ptr,
                        config_data.len(),
                    );
                    (*context.as_mut_ptr()).extradata = extradata_ptr;
                    (*context.as_mut_ptr()).extradata_size = config_data.len() as i32;
                }
            }
        }
        Err(_) => {
            info!("No audio config received, proceeding with ADTS-only frames");
        }
    }

    let mut decoder = context.decoder().audio().context("Failed to open AAC decoder")?;
    let mut resampler: Option<resampling::Context> = None;
    let mut frames_decoded: u64 = 0;

    // Target output format: S16LE, 44100Hz, Mono (matching phone's capture)
    let target_format = Sample::I16(ffmpeg::util::format::sample::Type::Packed);
    let target_rate = 44100u32;
    let target_layout = ChannelLayout::MONO;

    info!("Audio decoder ready, processing frames...");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Receive AAC frame data
        let aac_data = match audio_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(data) => data,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("Audio input channel disconnected");
                break;
            }
        };

        // Create an FFmpeg packet from the raw AAC data (with ADTS header)
        let packet = ffmpeg::Packet::copy(&aac_data);

        // Send to decoder
        if let Err(e) = decoder.send_packet(&packet) {
            debug!("AAC decode send error (may be transient): {}", e);
            continue;
        }

        // Drain decoded frames
        let mut decoded_frame = ffmpeg::frame::Audio::empty();
        while decoder.receive_frame(&mut decoded_frame).is_ok() {
            frames_decoded += 1;

            let src_format = decoded_frame.format();
            let src_rate = decoded_frame.rate();
            let src_layout = decoded_frame.channel_layout();

            // Initialize resampler on first frame if format doesn't match target
            let needs_resample = src_format != target_format
                || src_rate != target_rate
                || src_layout != target_layout;

            if needs_resample {
                if resampler.is_none() {
                    info!(
                        "Audio: resampling {:?}/{}Hz/{:?} → {:?}/{}Hz/{:?}",
                        src_format, src_rate, src_layout,
                        target_format, target_rate, target_layout
                    );
                    let r = resampling::Context::get(
                        src_format,
                        src_layout,
                        src_rate,
                        target_format,
                        target_layout,
                        target_rate,
                    )
                    .context("Failed to create audio resampler")?;
                    resampler = Some(r);
                }

                if let Some(ref mut r) = resampler {
                    let mut output = ffmpeg::frame::Audio::empty();
                    if let Err(e) = r.run(&decoded_frame, &mut output) {
                        warn!("Audio resample error: {}", e);
                        continue;
                    }

                    // Extract PCM data from resampled frame
                    let pcm_data = output.data(0).to_vec();
                    if pcm_tx.try_send(pcm_data).is_err() {
                        debug!("Audio output channel full, dropping frame");
                    }
                }
            } else {
                // No resampling needed — data is already in target format
                let pcm_data = decoded_frame.data(0).to_vec();
                if pcm_tx.try_send(pcm_data).is_err() {
                    debug!("Audio output channel full, dropping frame");
                }
            }

            if frames_decoded % 1000 == 0 {
                debug!("Audio: {} frames decoded", frames_decoded);
            }
        }
    }

    // Flush decoder
    let _ = decoder.send_eof();
    let mut decoded_frame = ffmpeg::frame::Audio::empty();
    while decoder.receive_frame(&mut decoded_frame).is_ok() {
        frames_decoded += 1;
    }

    info!("Audio decoder: {} total frames decoded", frames_decoded);
    Ok(())
}
