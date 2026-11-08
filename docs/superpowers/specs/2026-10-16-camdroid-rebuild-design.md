# CamDroid Media and Product Rebuild Design

## Goal

Make CamDroid a reliable Android camera sender for the OPPO A5x 5G and an Intel i3-1115G4/8 GB Linux laptop, with a measured path toward 1080p60 at high bitrate, a hard sustained floor of 30 fps, no growing latency, and under 100 ms capture-to-V4L2 latency in suitable USB and Wi-Fi conditions. Build the Android experience first, then make the desktop companion reflect the same real state.

## Constraints and acceptance baseline

- The OPPO A5x 5G is the first physical-device target. Camera and codec support must be queried at runtime; 1080p60 is a target mode, not a promise before the device is tested.
- The laptop has an Intel i3-1115G4 and 8 GB DDR4. The desktop must retain a software-decoding fallback and bounded memory use; hardware decode is optional.
- Do not use an Android emulator for acceptance testing. Do not require a phone build or device run until the user connects the phone.
- USB uses ADB forwarding. Wi-Fi must support discovery and direct connection. The media transport must be replaceable so a datagram path can be used if TCP head-of-line blocking prevents the latency target.
- Video is the priority. Video queues must be bounded and drop stale frames instead of retaining old frames. Audio must have bounded buffering and follow an explicit media clock.
- Every reported capability and health value must come from the runtime pipeline, not a hard-coded UI estimate.
- Existing non-functional controls and settings are removed, disabled, or connected to real behavior; they must not remain deceptively actionable.

## Architecture

CamDroid will use a versioned session protocol with a reliable control/negotiation channel and a media transport abstraction. The first implementation keeps the existing TCP/ADB route to reduce migration risk, but removes blocking work from encoder callbacks and makes the media sender and receiver latest-frame aware. Wi-Fi transport is isolated behind the same interface so a UDP/QUIC/RTP-like implementation can be added and measured without changing camera, codec, or UI code.

The Android side is split into capability discovery, stream session state, capture/encode, media sender, and UI state. The desktop side is split into connection/session, packet dispatch, decode, timing, bounded frame delivery, and V4L2/PulseAudio output. Each session reports timestamps, sequence numbers, queue depth, drops, and measured durations.

### Media contract

Each video and audio unit carries:

- protocol version and session ID;
- media kind, sequence number, capture timestamp in a defined monotonic clock domain;
- keyframe/config/discontinuity flags;
- codec, profile/level, dimensions, frame rate, sample rate, and channel layout in configuration metadata;
- payload length with strict validation.

The receiver maintains a small target playout budget. It drops stale video before decoding when the queue exceeds the budget, resets on discontinuity/config change, and reports frame age and drops. Audio uses the same clock and applies bounded resampling or controlled drop/insert to prevent drift.

### Android product flow

The phone is the first polished product surface:

1. Onboarding grants permissions and explains USB/Wi-Fi pairing.
2. Home shows live capability/readiness state, a copyable/QR connection endpoint, and one truthful start/stop action.
3. Viewfinder shows the camera preview, verified stream mode, connection state, measured sender health, and only supported controls.
4. Settings controls persistent preferences that are actually consumed by the service; unsupported options are unavailable with an explanation.
5. The service continues in the foreground with an actionable notification and a clear reconnect state.

### Desktop product flow

After the phone and protocol are stable, the desktop GUI consumes the session state rather than inventing its own. It supports discovery, manual IP, and USB device selection, shows capability-aware connection choices, provides a real preview or an explicit output-only state, reports live health, and only exposes runtime changes that can be acknowledged and applied by both endpoints.

## Error and recovery model

- Capability negotiation rejects unsupported modes before starting a stream.
- Every session has explicit states: idle, discovering, connecting, negotiating, starting, streaming, degraded, reconnecting, stopped, and failed.
- A watchdog detects missing packets/heartbeats and closes half-open connections.
- A reconnect performs a new handshake and decoder reset. It does not append to old queues.
- Encoder restarts and camera switches send a discontinuity plus new configuration; the receiver acknowledges readiness before accepting normal frames.
- Slow consumers trigger frame-age based drops and a quality response. They never block the Android encoder callback.
- Errors are structured and user-facing text is derived from error codes; raw exception text remains diagnostic detail.

## Performance instrumentation

The pipeline must expose, at minimum:

- capture, encoded, sent, received, decoded, and output FPS;
- encoded bitrate and payload rate;
- capture-to-send, send-to-receive, receive-to-output, and total frame age;
- per-stage queue depth and dropped frame count;
- decoder errors, reconnect count/reason, current mode, battery, and thermal state where available;
- audio/video clock offset and audio buffer duration.

The phone UI may show a compact subset. The desktop diagnostics view can show the complete set.

## Out of scope for the first implementation pass

- Cloud relay, internet traversal, accounts, recording, filters, and multi-phone sessions.
- Guaranteeing 1080p60 on hardware whose Camera2 capabilities do not expose it.
- Replacing the Linux virtual camera stack with a platform-neutral driver.
- Treating an emulator as equivalent to the OPPO device.

