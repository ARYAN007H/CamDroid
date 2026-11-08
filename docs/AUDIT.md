# CamDroid Phase 0 Audit
[the laptop user is using has i31115g4 and 8gb ddr4 ram and no dedicated gpu]
**Date:** 2026-10-16  
**Scope:** static review of all Android, desktop Rust, Tauri, and protocol source. No code, build configuration, or runtime behaviour was changed as part of this audit. The conclusions below describe what the current source implements; they are not device measurements.

## Executive summary

CamDroid has the right broad split—hardware capture and encode on Android, native desktop receive/decode, and a virtual V4L2 device—but its latency and smoothness are not controlled end to end. The current receiver can buffer **120 encoded video frames** before it begins to drop frames: at 60 fps that is two seconds of avoidable latency. After decode it buffers eight more uncompressed 1080p YUYV frames (about 32 MiB), then deliberately sleeps to pace an already delayed FIFO.

The desktop path also CPU-converts every decoded frame to YUYV422 and allocates a new approximately 4 MiB vector for every 1080p frame. At 60 fps the V4L2 output alone writes about 249 MB/s and the conversion/copy path processes at least that much additional memory bandwidth. This is the most likely sustained-60-fps bottleneck on many laptops.

There is no media timestamp in the wire protocol. Android generates video and audio presentation timestamps, but drops them at the protocol boundary. The desktop supplies neither packet PTS nor a shared clock to FFmpeg or PulseAudio. As a result, video and audio run on independent queues with no A/V synchronization, latency measurement, drift correction, or stale-frame policy.

The phone UI has useful visual ingredients but mixes display state, configuration, and runtime state in a global singleton. Several controls are non-functional or misleading. The desktop GUI is an early scaffold: it has no live preview, hard-codes the connection configuration, and labels static configuration as live stream status.

The production target—1080p60 at high bitrate and under 100 ms—requires a pipeline redesign, not local patches. The first technical milestone should be measured, bounded, latest-frame video delivery and accurate telemetry. UI work should then present the real connection and media state, beginning with the phone as requested.

## Current effective media configuration

### Android sender

| Setting | Current source behaviour |
|---|---|
| Default codec | H.264 (`video/avc`). H.265 (`video/hevc`) and MJPEG (`video/mjpeg`) are offered without querying device codec support. |
| Default resolution | 1920×1080 (`1080p`). 2560×1440 and 3840×2160 are also exposed. |
| Default frame rate | 60 fps. Camera2 selects the first available FPS range whose upper bound equals 60, or a range that contains 60. It does not establish that the requested resolution can actually sustain 60 fps. |
| Default video bitrate | H.264: 6 Mb/s at 1080p, 12 Mb/s at 1440p, 20 Mb/s at 4K. H.265 uses 60% of those values; MJPEG uses twice those values. |
| Rate control | `MediaCodec` CBR. `EncoderConfig.adaptiveBitrate` exists but is never used. There is no network throughput measurement or adaptive controller. A low-battery callback halves the active encoder bitrate once. |
| Keyframe interval | 1 second (`KEY_I_FRAME_INTERVAL = 1`). A client can request a sync frame. |
| Video profile / level | **Not configured.** No AVC/HEVC profile, level, B-frame policy, encoder latency, operating rate, or device capability selection is set. The device chooses its default profile and level. |
| Audio | AAC-LC, 44.1 kHz, mono, 128 kb/s. PCM is captured from `AudioRecord`, encoded by `MediaCodec`, then wrapped in ADTS. |
| Actual start path | The phone starts its server when the foreground service starts, but starts camera and encoders only when a desktop TCP client connects. The service configuration comes from the phone's saved settings at that time. |

Important configuration mismatches:

- The desktop sends a `start` command with codec, resolution, fps, and audio settings, but Android does **not** handle `start`. The phone therefore does not negotiate or apply the desktop selection.
- The Android `audioEnabled`, `keepScreenOn`, `autoDiscovery`, and saved front-camera preferences are stored but not consistently consumed by the service. `audioEnabled` remains `true` in `CameraStreamingService`.
- The desktop GUI always connects with H.264, 1080p, 60 fps, and audio enabled. Its session configuration can diverge from the phone configuration.
- `CameraManager.getBestOutputSize()` is unused. The app never validates a camera/encoder combination before presenting 1080p60, 1440p60, or 4K60 as available.
- The displayed Android bitrate is a calculation that assumes every encoded frame is exactly 100 KB. It is not a measurement of encoder or network bitrate.

### Desktop receiver

| Setting | Current source behaviour |
|---|---|
| Default connection mode | mDNS discovery unless direct IP or USB/ADB is selected. |
| TCP | `TCP_NODELAY` is enabled at both ends. Every protocol packet is flushed immediately. |
| Video decode | FFmpeg software decode selected by the requested desktop codec. No hardware decode path is selected. |
| Video output | FFmpeg `swscale` converts each decoded frame to YUYV422, then writes it to `/dev/video10` through `v4l2loopback`. |
| Audio output | FFmpeg AAC decode and optional resample to 44.1 kHz mono S16LE, then `PulseAudio` null sink. |
| Video buffer capacities | 120 encoded frames between TCP reader and decoder; 8 decoded YUYV frames between decoder and V4L2 writer. |
| Audio buffer capacities | 240 encoded AAC packets and 240 decoded PCM packets. |
| Output pacing | The V4L2 writer sleeps to the configured frame interval after each write. It is FIFO-based and never discards old decoded frames to catch up. |

## End-to-end frame path and copies

### Video

```text
Camera sensor / HAL
  → Camera2 capture request
  → MediaCodec input Surface                         (surface path; no app CPU pixel copy)
  → MediaCodec encoded output ByteBuffer
  → new Android ByteArray(info.size)                 (copy + allocation per encoded output)
  → StreamServer synchronized socket write + flush   (encoder callback blocks here)
  → TCP / ADB forwarding / kernel socket buffers
  → desktop protocol read_packet Vec                 (allocation + read copy)
  → bounded encoded-video queue: 120 frames          (FIFO, drop only after it is full)
  → FFmpeg Packet::copy                              (encoded payload copied again)
  → FFmpeg software decoder
  → FFmpeg swscale: source pixel format → YUYV422    (CPU conversion)
  → new YUYV Vec, copied one row at a time            (~4.15 MB/frame at 1080p)
  → bounded decoded-frame queue: 8 frames            (FIFO)
  → blocking write_all to v4l2loopback
  → consuming video application
```

At 1920×1080, YUYV422 is 4,147,200 bytes per frame. 60 frames per second is roughly 249 MB/s of raw writes before considering decoder output, `swscale`, row copies, allocations, and kernel/device buffering.

### Audio

```text
AudioRecord PCM ByteArray
  → MediaCodec AAC input                              (PCM copied into encoder input buffer)
  → AAC output ByteArray                              (copy + allocation)
  → ADTS ByteArray                                    (another allocation/copy)
  → same synchronized TCP sender as video
  → desktop protocol Vec
  → encoded-audio queue: 240 packets
  → FFmpeg Packet::copy / AAC decode
  → optional resample, PCM Vec allocation
  → decoded-PCM queue: 240 packets
  → blocking PulseAudio Simple::write
```

### Bottlenecks and latency risks

1. **Encoded video queue is 120 frames.** It allows about 2 seconds of queued 60-fps video before the desktop starts dropping packets. Dropping only after a large FIFO has filled preserves stale frames and creates visible latency.
2. **CPU pixel conversion and allocation per frame.** `swscale` to YUYV422 plus a 4 MiB row-copy allocation per 1080p frame is expensive and causes allocator and cache pressure at 60 fps.
3. **V4L2 FIFO plus pacing sleep.** The writer waits for the next old frame, writes it, then sleeps. If decode falls behind, it cannot catch up and it adds latency instead of selecting the newest frame.
4. **Sender does synchronous per-packet socket I/O in the MediaCodec output callback.** `writePacket` allocates a header, writes, and flushes while holding a shared lock. A slow network, slow ADB tunnel, or blocked peer stalls video output production and serializes audio and heartbeat traffic behind it.
5. **TCP head-of-line blocking.** With a single stream, packet loss or a congested large video frame delays every later video, audio, heartbeat, and control packet.
6. **Large independent audio queues.** They can retain several seconds of audio. They neither track video nor use a target latency.
7. **Per-frame flushing and allocations.** Both endpoints allocate packet storage; the sender flushes each packet and shares the sender lock across video, audio, metadata, and heartbeat.
8. **No capability validation.** An unsupported codec or a camera mode that cannot deliver 60 fps can produce an unstable stream that the UI still describes as 60 fps.

The Android encoder callback also reads `MediaCodec` output using `info.size` but does not explicitly apply `BufferInfo.offset` before copying. That must be corrected and tested during the media rewrite so every packet contains exactly the valid encoded range.

## Timestamps, A/V sync, and telemetry

### Current behaviour

- Video receives `MediaCodec.BufferInfo.presentationTimeUs`; audio uses `System.nanoTime() / 1000` when PCM is queued and receives an encoded PTS. Both PTS values are passed to Android callbacks.
- `CameraStreamingService` discards both PTS values before packets are sent. `VIDEO_FRAME` and `AUDIO_FRAME` carry only encoded bytes.
- The desktop constructs FFmpeg packets without PTS/DTS. It cannot relate decoded output to capture time.
- There is no shared monotonic clock, sequence number, capture timestamp, sender queue timestamp, arrival timestamp, frame age metric, jitter metric, dropped-frame counter, or latency budget.
- Audio and video are decoded and played in separate threads and queues. PulseAudio playback timing is independent of video/V4L2 output. There is no synchronization, drift correction, or recovery after a gap.
- Android's `currentFps` counts encoded non-config video callbacks per second. It does not measure capture FPS, receiver FPS, or displayed FPS. Its bitrate display is synthetic, as noted above.
- The desktop GUI polls only static session configuration and battery. Its FPS and Mbps fields never receive stream metrics.

### Consequence

The system cannot prove or enforce the requested sub-100-ms target. It can accumulate delay in TCP, queues, decoder reordering, V4L2, and PulseAudio without an observable age budget. A/V offset can drift indefinitely.

## Slow network, disconnect, and reconnect behaviour

### Slow network or slow desktop

- Android does not queue video itself; it blocks the `MediaCodec` callback on a synchronized, flushing TCP write. Backpressure therefore reaches the encoder thread, while TCP kernel buffers may already contain old media.
- Desktop TCP reading uses `try_send`. It drops a packet only after its large queue is full. By then, most retained frames are stale.
- Desktop decoder and V4L2 queues similarly drop new frames only when full, retaining old frames that increase latency.
- The advertised adaptive bitrate mode does not exist in the runtime. `set_bitrate` sets a bitrate immediately, and setting it to zero does not re-enable an adaptation algorithm.
- There is no congestion signal, quality ladder, loss estimate, or keyframe-aware recovery.

### Disconnect

- Android catches read/write exceptions and changes the UI status to “waiting”, but its read loop does not close/reset the disconnected client at loop exit. Subsequent video sends keep attempting a failed output stream until a new client replaces it.
- The desktop reader exits on a protocol or socket error but does not set the shared shutdown flag. `CamdroidSession::status()` therefore continues to report `connected: true` and `streaming: true` until local shutdown.
- Neither side implements the 10-second no-packet timeout promised by `PROTOCOL.md`; heartbeats are sent but no last-received watchdog exists.
- Blocking reads can wait indefinitely on a half-open TCP connection.

### Reconnect

- Android's accept loop can accept a later connection and closes the previous client before replacing it.
- The desktop client does not reconnect automatically. It must be restarted or reconnected through the GUI.
- There is no session identifier, resume state, configuration acknowledgement, decoder reset protocol, or explicit recovery sequence after a disconnect or encoder restart.
- Changing codec or resolution on the phone restarts the sender pipeline, but the desktop decoder/output pipeline is created once. It is not rebuilt for the new codec or dimensions, making those controls unsafe in the current GUI.

## Protocol audit

The packet envelope is small and clear: one type byte, four little-endian length bytes, and payload. Its central missing element is media timing and state semantics.

| Topic | Specification / implementation gap |
|---|---|
| Versioning | The document describes protocol v1.0; Android advertises `1.0`; desktop handshake advertises `2.0` without a compatibility check. |
| Start negotiation | The document says the client start command configures the stream. Android ignores `start`, so connection configuration is not negotiated. |
| Video framing | The document requires one NAL unit per packet and Annex B framing. Android forwards each raw `MediaCodec` output buffer without converting or validating its framing, and does not specify profile/level or keyframe flags in packet metadata. |
| Timing | No frame timestamp, sequence number, duration, discontinuity, or clock definition is on the wire. |
| Errors and ACKs | The document describes error metadata, but Android does not send command acknowledgement or error packets. |
| Heartbeats | The document promises a 10-second death timeout. Neither implementation enforces it. |
| Reconfiguration | New codec configuration is sent, but there is no transactional restart state, dimension/profile metadata, receiver acknowledgement, or decoder/output rebuild contract. |
| Security | The protocol is unauthenticated and unencrypted. This is acceptable only as an explicitly trusted local-network development assumption; it must be a deliberate product decision before release. |

## UI/UX audit

### Android onboarding

- It makes unverified “4K @ 60 FPS” and ultra-low-latency promises before capability checks or a connection test.
- Users can skip onboarding without a clear readiness result. It does not pair a phone with a desktop, show the current network, or verify camera/microphone permissions after the request.
- Connection guidance is prose only. It does not provide a QR pairing route, an IP copy action, USB readiness checks, or a first successful-stream confirmation.

### Android home

- The primary button says “Stop Streaming” while streaming, but its callback always navigates to the viewfinder; it does not stop the service from this screen.
- Wi-Fi Auto-Discover, USB, and Direct IP cards are tappable but have empty click handlers. They appear actionable but do nothing.
- The IP address is calculated once with `remember`, so a Wi-Fi change is not reflected. There is no copy, QR, network quality indicator, or explanation of the actual desktop next step.
- It presents saved preferences as the active stream configuration even when the service has not negotiated with the desktop or a live stream may have a different state.

### Android viewfinder

- Mirror is wired to the flash callback, so pressing Mirror toggles torch state. `CameraManager.setMirror` itself stores a flag but applies no transform.
- Tap-to-focus draws a focus ring but ignores the tap coordinates and sends ordinary continuous auto-focus. The interaction implies a result it does not perform.
- The `0.5x` zoom option is always shown despite the stream control clamping zoom to a minimum of `1x`; it is misleading on devices without an ultra-wide path.
- Focus, manual exposure, white balance, torch, and stabilization are shown without checking individual camera capabilities. Front camera and device differences are not communicated.
- The UI reports synthetic bitrate and sender callback FPS rather than camera, encoder, network, receiver, and output health. It cannot explain a bad stream.
- Five labeled controls plus a shutter compete in a portrait bottom bar; the hard-coded sheet positioning can conflict with small screens and system insets.
- The screen owns service start/stop through intents and a global callback singleton, which makes lifecycle transitions and error recovery hard to represent clearly.

### Android settings and about

- Audio streaming, keep-screen-on, auto-discovery, server port, and front-camera settings are not all applied by the current service. Port selection is explicitly a TODO; the screen presents it as an available preference.
- Frame-rate selection is a single row that silently toggles 30/60. It offers no mode compatibility, bandwidth estimate, codec profile, bitrate, latency mode, or battery/thermal trade-off.
- Settings do not expose the only controls that matter for the target outcome: verified capability, quality ladder, measured link quality, latency mode, and recovery policy.
- The About screen repeats unsupported claims such as “~30ms” and device-independent 4K60. It should describe verified capabilities and link to diagnostics instead.

### Desktop connection screen

- It hard-codes H.264, 1080p, 60 fps, and audio on every connection. Users cannot choose a mode or see the phone's reported capabilities before connecting.
- Auto-discovery has a five-second blocking operation with no true cancellation. The Cancel button only replaces the visible content; it does not cancel the discovery request.
- Retry after an empty discovery result dispatches a click to an element that has already been removed, so it is not a reliable retry path.
- Manual IP has only empty/non-empty validation. It does not validate an address or port, save recent devices, offer paste-friendly pairing, or describe whether the phone server is running.
- USB chooses the first ADB device silently. There is no device chooser, diagnostic state, authorization help, or cable/ADB readiness view.
- Device/error strings are inserted via `innerHTML` without escaping. Apart from being unsafe for untrusted local mDNS metadata, it makes error presentation inconsistent and overly technical.

### Desktop dashboard

- It has no live preview; it presents a placeholder instead of the actual stream the user is controlling.
- FPS and bitrate show `--` forever. The backend exposes no live performance metrics; only battery and device name are updated every two seconds.
- The dashboard uses optimistic controls and logs errors only to the developer console. There is no in-product confirmation, disabled state, restart progress, unsupported-control explanation, or recovery affordance.
- Changing codec or resolution updates a badge even though the receiver pipeline is not rebuilt. This can leave the UI claiming one configuration while the decoder/output path still uses another.
- It has no audio control, target FPS control, bitrate/quality selector, latency mode, device capability awareness, or persistent connection profile.
- The visual system is a polished-looking scaffold, but it depends heavily on emojis and direct DOM string templates; it lacks responsive breakpoints, keyboard/focus treatment, accessible labels, and a coherent information hierarchy for troubleshooting.

## Ranked fix list

The expected impacts are engineering expectations to validate with capture-to-display and A/V measurements. They are not guarantees until tested on representative phones, Wi-Fi conditions, USB, and Linux hardware.

1. **Define a measured latency contract and replace FIFO buffering with a bounded latest-frame policy.**
   - Add capture timestamps, sequence numbers, sender/receiver queue depth, frame age, encode/decode/output duration, drops, and A/V offset metrics. Cap each video stage at 1–3 frames and always discard stale frames in favour of the newest decodable frame.
   - Expected impact: removes the current seconds-long queue failure mode and makes sub-100-ms latency observable and enforceable.

2. **Redesign the media protocol and session state machine before changing UI.**
   - Create a versioned handshake that negotiates actual phone capabilities and selected codec/resolution/fps/bitrate. Include media timestamps, keyframe/config/discontinuity information, acknowledgements, error codes, heartbeat watchdogs, reconnect policy, and explicit encoder-reconfiguration transactions.
   - For Wi-Fi, evaluate a datagram transport such as QUIC or RTP-like UDP for media so one lost packet does not stall later frames; retain a reliable control channel. USB can continue through ADB with the same session semantics.
   - Expected impact: stable recovery, correct runtime configuration, measurable timing, and removal of TCP head-of-line stalls from the wireless media path.

3. **Make Android a latency-aware sender.**
   - Keep Camera2 → encoder Surface capture, but query camera and codec capabilities for every presented mode. Select a verified 1080p60 H.264 baseline; configure profile/level, CBR/VBR policy, keyframe cadence, low-latency knobs when supported, and thermal/network quality ladders explicitly.
   - Move socket writes out of the `MediaCodec` callback into a small, drop-old media sender queue. Reuse buffers where possible, avoid per-frame header allocation, correct `BufferInfo.offset` handling, and keep control traffic independent from media.
   - Expected impact: protects hardware encoding from transport stalls and gives a feasible high-bitrate 1080p60 sender on supported phones.

4. **Replace the desktop video output hot path.**
   - Remove the 120-frame queue, eliminate deliberate FIFO pacing, reuse decode/output buffers, and write the newest frame to V4L2. Prefer hardware decode and an efficient compatible output format when available; retain a well-profiled software fallback with a fixed buffer pool.
   - Measure CPU conversion cost and V4L2 write blocking on the target laptop. Make decoder reinitialization a first-class operation after an acknowledged reconfiguration.
   - Expected impact: the largest gain toward sustained 1080p60 and the largest reduction in output-side delay and memory churn.

5. **Make audio follow the video clock.**
   - Use protocol timestamps and a defined playout delay; use small bounded audio buffers, track clock drift, and resample or drop/insert safely to maintain A/V alignment. Do not inject periodic silence merely because a queue timeout occurred without considering the shared media timeline.
   - Expected impact: prevents growing lip-sync error and makes audio behaviour predictable under jitter.

6. **Build a real diagnostics and recovery layer.**
   - Expose measured capture/encode/send/receive/decode/output FPS, bitrate, frame age, jitter, queue depth, dropped frames, Wi-Fi signal/link type, thermal state, and reconnect reason. Implement timeout-driven reconnect and keyframe recovery.
   - Expected impact: turns “not smooth” into a diagnosable condition and supports safe adaptation instead of silent degradation.

7. **Redesign the Android experience around a short, verified streaming workflow.**
   - Phone first: onboarding should grant permissions, validate camera modes, show a shareable pairing endpoint/QR, start a visible server state, then move to a focused viewfinder with truthful health and explicit controls. Make unsupported features unavailable, fix mirror/tap-focus, and remove non-functional cards/settings.
   - Expected impact: a dependable production workflow and a phone UI that communicates actual stream state instead of promises.

8. **Build the desktop product after the phone and protocol states are stable.**
   - Replace the Tauri scaffold with a capability-aware pairing flow, explicit connection profiles, live preview, real metrics, quality/latency controls, feedback for every command, and recovery states. Do not expose runtime codec/resolution switches until the session protocol and decoder restart path support them safely.
   - Expected impact: a coherent companion application whose UI matches the backend state.

9. **Add performance gates before declaring the target achieved.**
   - Capture end-to-end LED/clock latency, 60-fps frame pacing, 30-fps minimum, tail latency, CPU/GPU usage, memory, dropped frames, recovery time, and A/V offset. Test USB, clean 5 GHz/6 GHz Wi-Fi, congested Wi-Fi, screen dim/background, orientation/camera switches, and thermal soak.
   - Expected impact: replaces subjective smoothness with release criteria for the stated 1080p60/high-bitrate/<100-ms target.

## Recommended next phase boundary

Do not begin the visual redesign as an isolated pass. First establish the new protocol/session contract, instrument the pipeline, and build the Android sender's bounded media path. Once the phone can report truthful capability and stream health, redesign the Android pairing, home, and viewfinder flow around those states. The desktop application can then consume the same contract and metrics without duplicating assumptions.
