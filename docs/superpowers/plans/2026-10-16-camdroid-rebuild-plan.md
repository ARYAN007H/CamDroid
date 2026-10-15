# CamDroid Rebuild Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild CamDroid’s media/session foundation and Android-first product flow so it can be measured and improved toward 1080p60, sustained 30 fps minimum, no growing latency, and under 100 ms end-to-end latency on the OPPO A5x 5G and Intel i3-1115G4 laptop.

**Architecture:** Introduce a versioned, timestamped session protocol and transport abstraction; keep TCP/ADB as the first route while isolating Wi-Fi media transport. Make Android encoding and desktop receiving asynchronous with bounded latest-frame queues, explicit reconfiguration, and live telemetry. Polish Android around verified runtime capabilities first, then rebuild the Tauri companion around the same session state.

**Tech Stack:** Kotlin, Camera2, MediaCodec, Jetpack Compose, DataStore, coroutines; Rust, FFmpeg, crossbeam channels, V4L2, PulseAudio; Tauri 2, TypeScript, Vite; custom binary protocol.

**Spec:** `docs/superpowers/specs/2026-10-16-camdroid-rebuild-design.md`

## Global Constraints

- The primary phone target is OPPO A5x 5G; discover camera/codec capabilities at runtime.
- The primary desktop target is an Intel i3-1115G4 laptop with 8 GB RAM; preserve a software fallback and bounded memory use.
- Do not use an Android emulator for acceptance testing; physical-phone validation begins only after the user connects the phone.
- Video queues use a small bounded latest-frame policy; no stage may retain seconds of stale video.
- Media callbacks never perform blocking socket I/O.
- Every media unit has a sequence number, monotonic capture timestamp, and configuration/discontinuity semantics.
- Unsupported modes and controls are unavailable or explained; UI values must reflect measured runtime state.
- Each task follows TDD: write the failing unit/integration test, observe failure, implement the smallest change, rerun targeted tests, then run the task suite.

## Review Focus

- A slow/blocked TCP peer must not block Android’s encoder callback or cause unbounded latency; test bounded sender behavior.
- A full receiver queue must discard the oldest stale video and retain the newest decodable frame; test frame age and ordering.
- Mid-stream codec/resolution/camera changes must reset decoder/output state before accepting new frames; test discontinuity handling.
- A half-open connection must transition to reconnecting and clear old queues; test watchdog behavior.
- A device without 60-fps or a requested codec must receive an actionable unsupported-mode result and never be shown as active; test capability filtering.

## Delivery boundaries

Tasks 1–6 are the latency-critical media foundation. Tasks 7–9 make Android the first production surface. Tasks 10–11 build the desktop companion only after the shared state contract exists. No task requires a phone or emulator build until Task 9’s device validation gate.

### Task 1: Establish isolated implementation workspace and protocol test baseline

**Files:**
- Create: `docs/superpowers/sdd/` plan workspace artifacts as required by the execution skill.
- Modify: `desktop/src/protocol/wire.rs` tests only when needed to pin compatibility behavior.
- Create: `desktop/src/protocol/v2.rs` and `desktop/src/protocol/v2_tests.rs` (or an inline Rust test module if that matches the existing project convention).
- Create: `android/app/src/test/java/com/camdroid/network/ProtocolV2Test.kt`.

**Interfaces:**
- Consumes: existing `Packet`, `PacketType`, `ControlCommand`, and `protocol/PROTOCOL.md`.
- Produces: shared protocol constants and serialization test vectors for Tasks 2–5.

- [ ] **Step 1: Record baseline behavior**
  - Inspect `git status`, existing tests, and protocol fixtures without compiling Android or starting an emulator.
  - Expected: clean baseline except the audit/planning documents; existing Rust protocol tests are identified.
- [ ] **Step 2: Write failing protocol tests**
  - Cover a timestamped video packet round trip, sequence/session fields, configuration flags, maximum payload rejection, truncated packet rejection, and compatibility rejection for unsupported versions.
- [ ] **Step 3: Run the targeted Rust protocol tests**
  - Run: `cargo test --manifest-path desktop/Cargo.toml protocol`
  - Expected: new v2 tests fail because the v2 types/codec do not exist; existing v1 tests continue to identify their baseline result.
- [ ] **Step 4: Implement protocol v2 value types and codec**
  - Define exact little-endian wire encoding, `SessionId`, `MediaTimestamp`, `MediaFlags`, `MediaKind`, `StreamConfig`, and strict payload limits. Keep control JSON as a typed payload inside the versioned envelope.
- [ ] **Step 5: Mirror the packet vectors in Kotlin**
  - Implement the same encode/decode contract in Android tests and production protocol code, with explicit byte-order and offset handling.
- [ ] **Step 6: Run targeted tests and compare vectors**
  - Run: `cargo test --manifest-path desktop/Cargo.toml protocol` and the Android unit test command only when the user has authorized the first Android build.
  - Expected: Rust vectors pass; Android execution is recorded as deferred until the physical phone/build phase if the user still does not want compilation.
- [ ] **Step 7: Commit**
  - Commit message: `feat: define timestamped media protocol v2`.

### Task 2: Add capability negotiation and explicit session state

**Files:**
- Modify: `desktop/src/lib.rs`, `desktop/src/connection/tcp_client.rs`, `desktop/src/protocol/mod.rs`.
- Create: `desktop/src/session.rs` and tests in `desktop/src/session_tests.rs`.
- Modify: `android/app/src/main/java/com/camdroid/network/StreamServer.kt`, `ProtocolHandler.kt`.
- Create: `android/app/src/main/java/com/camdroid/network/SessionState.kt` and pure unit tests.

**Interfaces:**
- Consumes: Task 1 v2 envelope and configuration types.
- Produces: `SessionState`, `CapabilitySet`, `NegotiatedStream`, `SessionEvent`, and explicit transitions used by sender, receiver, and UI.

- [ ] **Step 1: Write failing state-machine tests**
  - Assert idle → connecting → negotiating → streaming, invalid transitions, unsupported mode rejection, reconnect clearing queues, and reconfiguration requiring acknowledgement.
- [ ] **Step 2: Run tests and observe failure**
  - Run: `cargo test --manifest-path desktop/Cargo.toml session`
  - Expected: state types and transitions are missing.
- [ ] **Step 3: Implement the shared negotiation model**
  - Add version/session handshake, capability intersection, selected codec/resolution/fps/bitrate/audio, and structured error codes. Android must answer the desktop’s start request or reject it explicitly.
- [ ] **Step 4: Route current TCP handshake through the state machine**
  - Remove the current “send start and assume” behavior. Do not start decoder/output threads until `NegotiatedStream` is acknowledged.
- [ ] **Step 5: Run focused and existing protocol tests**
  - Expected: unsupported configurations fail before media starts; valid H.264 modes reach streaming.
- [ ] **Step 6: Commit**
  - Commit message: `feat: add negotiated streaming session state`.

### Task 3: Make Android capture/encode and send nonblocking

**Files:**
- Modify: `android/app/src/main/java/com/camdroid/encoder/VideoEncoder.kt`.
- Modify: `android/app/src/main/java/com/camdroid/encoder/AudioEncoder.kt`.
- Modify: `android/app/src/main/java/com/camdroid/CameraStreamingService.kt`.
- Modify: `android/app/src/main/java/com/camdroid/network/StreamServer.kt`.
- Create: `android/app/src/main/java/com/camdroid/network/MediaSender.kt` and tests for queue/drop policy.
- Modify: `android/app/src/main/java/com/camdroid/encoder/EncoderConfig.kt`.

**Interfaces:**
- Consumes: Task 2 `NegotiatedStream` and Task 1 timestamped packet types.
- Produces: `MediaSender.offerVideo(frame)`, `MediaSender.offerAudio(frame)`, `MediaSender.stats`, and `EncoderConfig` with verified profile/latency/keyframe settings.

- [ ] **Step 1: Write failing sender queue tests**
  - Assert a blocked output does not block `offerVideo`, the queue stays within its configured capacity, the oldest stale video is dropped, audio remains bounded, and config/keyframe packets are never discarded ahead of media.
- [ ] **Step 2: Run queue tests to verify failure**
  - Run the Android pure/unit test target once Android compilation is authorized; before that, keep the tests as the planned gate and do not invoke Gradle.
  - Expected: no `MediaSender` exists.
- [ ] **Step 3: Implement sender and independent control writer**
  - Copy encoded output using `BufferInfo.offset` and `size`; enqueue immutable frame metadata; perform socket writes on dedicated media/control workers; remove per-frame callback flushes and header allocations where safe.
- [ ] **Step 4: Configure verified low-latency encoder values**
  - Set explicit H.264 profile/level only when the queried codec advertises them; preserve 1-second keyframes initially; gate low-latency MediaFormat keys by API/device support; reject unsupported MJPEG/HEVC instead of attempting them blindly.
- [ ] **Step 5: Add Android sender telemetry**
  - Track capture/encoded/sent FPS, bitrate from actual payload bytes, queue depth, frame age, and drops using a monotonic clock.
- [ ] **Step 6: Run focused tests and commit**
  - Commit message: `perf: decouple Android media sending from encoder callbacks`.

### Task 4: Implement receiver latest-frame delivery and timestamps

**Files:**
- Modify: `desktop/src/connection/tcp_client.rs`.
- Create: `desktop/src/media_queue.rs` and tests.
- Modify: `desktop/src/video/decoder.rs`.
- Modify: `desktop/src/audio/aac_decoder.rs`.
- Modify: `desktop/src/lib.rs`.

**Interfaces:**
- Consumes: Task 1 media packets and Task 2 negotiated stream.
- Produces: bounded `LatestVideoQueue`, bounded audio queue, decoder timestamps, `ReceiverStats`, and discontinuity reset hooks.

- [ ] **Step 1: Write failing queue/timing tests**
  - Test capacity of 3 video frames, oldest-frame replacement, sequence gap accounting, frame-age calculation, bounded audio duration, and reset on session/config discontinuity.
- [ ] **Step 2: Run tests and observe failure**
  - Run: `cargo test --manifest-path desktop/Cargo.toml media_queue`
  - Expected: queue and timing types are missing.
- [ ] **Step 3: Implement bounded media queues**
  - Replace the 120-frame video channel with a latest-frame queue. Preserve config/keyframe ordering and never let metadata/control starvation block media dispatch.
- [ ] **Step 4: Apply packet timestamps to FFmpeg and output timing**
  - Set PTS/time base on packets, propagate decoder timestamps, and expose frame age. Remove the 500 ms receive waits as a latency mechanism.
- [ ] **Step 5: Add receiver and A/V telemetry**
  - Track received/dropped/decoded/output counts, queue durations, and audio/video clock offset. Do not claim sync until both streams use the same protocol clock.
- [ ] **Step 6: Run desktop unit tests and commit**
  - Commit message: `perf: bound desktop media queues and preserve timestamps`.

### Task 5: Reduce desktop output cost and support safe reconfiguration

**Files:**
- Modify: `desktop/src/video/decoder.rs`.
- Modify: `desktop/src/video/v4l2_output.rs`.
- Create: `desktop/src/video/frame_pool.rs` and tests.
- Modify: `desktop/src/audio/pulse_output.rs`.
- Modify: `desktop/src/lib.rs` session lifecycle.

**Interfaces:**
- Consumes: Task 4 `LatestVideoQueue`, timestamps, `NegotiatedStream`, and discontinuity events.
- Produces: reusable decoded/output frame buffers, output health, and a decoder/output restart operation for codec or dimension changes.

- [ ] **Step 1: Write failing frame-pool and pacing tests**
  - Assert reuse after release, correct stride handling, no unbounded allocations over a simulated 60-frame run, and newest-frame selection when output is slower than input.
- [ ] **Step 2: Run focused tests and observe failure**
  - Run: `cargo test --manifest-path desktop/Cargo.toml video`
  - Expected: frame pool and latest-frame output behavior are absent.
- [ ] **Step 3: Implement reusable conversion/output buffers**
  - Reuse YUYV storage, avoid `Vec::with_capacity` per frame, and keep conversion isolated so an optional hardware decode/format path can be added later. Preserve a software fallback for the i3 laptop.
- [ ] **Step 4: Replace FIFO sleep pacing**
  - Output the freshest eligible frame against the negotiated clock; do not sleep after a stale frame. Report blocking writes and output FPS.
- [ ] **Step 5: Implement transactional reconfiguration**
  - Stop/reset decoder and V4L2 format only after a new configuration/discontinuity is received and acknowledged. Reset audio format/PulseAudio stream when required.
- [ ] **Step 6: Run video/audio tests and commit**
  - Commit message: `perf: reuse desktop output buffers and reset safely`.

### Task 6: Add disconnect, watchdog, reconnect, and transport abstraction

**Files:**
- Modify: `desktop/src/connection/tcp_client.rs`, `desktop/src/session.rs`, `desktop/src/lib.rs`.
- Modify: `android/app/src/main/java/com/camdroid/network/StreamServer.kt` and `CameraStreamingService.kt`.
- Create: `desktop/src/connection/transport.rs` and transport tests.
- Modify: `protocol/PROTOCOL.md` to the negotiated v2 behavior.

**Interfaces:**
- Consumes: Tasks 1–5 session, queue, and reset interfaces.
- Produces: `Transport` abstraction, watchdog events, reconnect backoff, and explicit session recovery.

- [ ] **Step 1: Write failing watchdog/reconnect tests**
  - Test heartbeat timeout, half-open read closure, old queue clearing, reconnect backoff bounds, and a new handshake after reconnect.
- [ ] **Step 2: Run tests and observe failure**
  - Run: `cargo test --manifest-path desktop/Cargo.toml connection session`
  - Expected: current reader exits without changing session state and no reconnect policy exists.
- [ ] **Step 3: Implement TCP transport behind the interface**
  - Add read/write deadlines, last-received tracking, clean close, and separate control/media channels where supported. Preserve ADB by connecting it through the same TCP transport.
- [ ] **Step 4: Implement Android disconnect cleanup**
  - Close/reset the client socket on read failure, stop media workers, keep the server discoverable, and allow a new client to negotiate cleanly.
- [ ] **Step 5: Add a Wi-Fi datagram feasibility boundary**
  - Define the interface and telemetry needed for a future UDP/QUIC/RTP-like media transport. Do not add a dependency or claim lower latency until a later measured task selects one.
- [ ] **Step 6: Update the protocol document and commit**
  - Commit message: `feat: add bounded session recovery and transport abstraction`.

### Task 7: Implement truthful Android capability and runtime state

**Files:**
- Modify: `android/app/src/main/java/com/camdroid/camera/CameraManager.kt`.
- Modify: `android/app/src/main/java/com/camdroid/encoder/EncoderConfig.kt`.
- Create: `android/app/src/main/java/com/camdroid/capability/CapabilityRepository.kt` and tests.
- Modify: `android/app/src/main/java/com/camdroid/StreamingController.kt`.
- Modify: `android/app/src/main/java/com/camdroid/data/SettingsRepository.kt`.

**Interfaces:**
- Consumes: Task 2 negotiated stream and Task 3 sender stats.
- Produces: capability-aware `CameraMode`, `VideoMode`, `AudioMode`, and a lifecycle-safe `StreamUiState` consumed by Android screens.

- [ ] **Step 1: Write failing capability tests**
  - Test filtering of unsupported camera sizes/FPS ranges/codecs, bitrate defaults by mode, and persistence round trips for only settings the service consumes.
- [ ] **Step 2: Implement runtime capability discovery**
  - Query Camera2 stream sizes/FPS and MediaCodec capabilities. Remove unsupported choices from negotiation and represent unavailable controls explicitly.
- [ ] **Step 3: Replace global callback state with service-backed state**
  - Use a state model that distinguishes idle, waiting, negotiating, streaming, degraded, and failed, carrying measured stats and last error.
- [ ] **Step 4: Wire all persisted settings**
  - Make audio, port, auto-discovery, screen-on, camera selection, and quality settings either operational or remove them from the UI. Ensure desktop-negotiated values are reflected back.
- [ ] **Step 5: Run pure Android tests and commit**
  - Commit message: `feat: expose verified Android capabilities and stream state`.

### Task 8: Fix and polish the Android onboarding, home, settings, and viewfinder

**Files:**
- Modify: `android/app/src/main/java/com/camdroid/ui/navigation/NavGraph.kt`.
- Modify: `android/app/src/main/java/com/camdroid/ui/screens/OnboardingScreen.kt`.
- Modify: `android/app/src/main/java/com/camdroid/ui/screens/HomeScreen.kt`.
- Modify: `android/app/src/main/java/com/camdroid/ui/screens/SettingsScreen.kt`.
- Modify: `android/app/src/main/java/com/camdroid/ui/screens/ViewfinderScreen.kt`.
- Modify: `android/app/src/main/java/com/camdroid/ui/components/Components.kt` and theme files as needed.

**Interfaces:**
- Consumes: Task 7 `StreamUiState`, capabilities, endpoint, and telemetry.
- Produces: a phone workflow with no empty click handlers or misleading controls.

- [ ] **Step 1: Write UI/state tests for the highest-risk interactions**
  - Cover start/stop idempotence, permission denial, endpoint copy/QR state, unsupported control visibility, mirror command routing, tap-focus coordinate routing, and reconnect/error rendering.
- [ ] **Step 2: Implement onboarding and home readiness flow**
  - Show permissions, network/endpoint, verified modes, copy/share actions, connection status, and a single start/stop action tied to service state.
- [ ] **Step 3: Implement truthful settings**
  - Replace TODO/no-op rows with capability-backed selectors, explicit quality/latency descriptions, and settings that are applied by the service.
- [ ] **Step 4: Implement viewfinder controls**
  - Fix mirror and tap-focus routing; make zoom ranges device-aware; expose real sender health; handle insets, dim mode, and service errors consistently.
- [ ] **Step 5: Review accessibility and small-screen layout**
  - Add content descriptions, minimum touch targets, stable state labels, and layout behavior for small portrait screens.
- [ ] **Step 6: Run Android unit/UI tests when build authorization and phone are available; commit**
  - Commit message: `feat: polish Android streaming workflow`.

### Task 9: Physical OPPO validation gate

**Files:**
- Modify: `docs/AUDIT.md` with measured results only.
- Create: `docs/validation/oppo-a5x-5g.md`.

**Interfaces:**
- Consumes: Tasks 3, 7, and 8 on the connected phone.
- Produces: measured supported modes and a go/no-go decision for 1080p60/high bitrate.

- [ ] **Step 1: Connect the OPPO and authorize USB debugging**
  - Do not use an emulator.
- [ ] **Step 2: Build/install the debug APK and run capability discovery**
  - Record Android version, camera sizes/FPS, encoder support, thermal/battery behavior, and permissions.
- [ ] **Step 3: Measure USB and Wi-Fi**
  - Record sustained FPS, frame age percentiles, bitrate, queue depth, drops, CPU/memory, reconnect behavior, and A/V offset at 1080p30 and the highest stable mode.
- [ ] **Step 4: Adjust only measured device profile values**
  - Do not lower the UI claim silently; report unsupported target modes explicitly.
- [ ] **Step 5: Commit validation record**
  - Commit message: `docs: record OPPO A5x 5G streaming baseline`.

### Task 10: Rebuild desktop session/diagnostics integration

**Files:**
- Modify: `desktop-tauri/src-tauri/src/commands.rs` and `lib.rs`.
- Modify: `desktop-tauri/src/pages/connection.ts`.
- Modify: `desktop-tauri/src/pages/dashboard.ts`.
- Modify: `desktop-tauri/src/main.ts`.
- Modify: `desktop-tauri/src/style.css`.

**Interfaces:**
- Consumes: Tasks 2, 4, 5, and 6 `SessionState`, `ReceiverStats`, capabilities, and recovery events.
- Produces: capability-aware connection and a dashboard that displays live state and real preview/output status.

- [ ] **Step 1: Write failing command/state tests**
  - Cover connection parameters from negotiated capabilities, structured errors, status updates, disconnect transition, and safe codec/resolution changes.
- [ ] **Step 2: Implement typed Tauri state events**
  - Replace static polling-only `SessionStatus` with serialized session/telemetry events; avoid holding a mutex during blocking connection work.
- [ ] **Step 3: Rebuild connection flow**
  - Add cancellable discovery, IP/port validation, USB device selection, saved profiles, and user-facing recovery messages.
- [ ] **Step 4: Rebuild dashboard around live telemetry**
  - Remove placeholder FPS/bitrate fields, show real queue/drop/latency state, and disable controls that cannot be safely applied.
- [ ] **Step 5: Add preview/output status and responsive accessibility**
  - Show whether the Linux virtual device is ready and make the layout keyboard/focus and small-window friendly.
- [ ] **Step 6: Run frontend/Rust tests and commit**
  - Commit message: `feat: connect desktop UI to live streaming state`.

### Task 11: Performance gates and release documentation

**Files:**
- Create: `docs/validation/performance-matrix.md`.
- Modify: `README.md` and `protocol/PROTOCOL.md`.
- Add tests/benchmarks alongside the media queue, decoder, and session modules where supported.

**Interfaces:**
- Consumes: all production telemetry and the physical-device baseline from Task 9.
- Produces: repeatable release gates and truthful user documentation.

- [ ] **Step 1: Define measured gates**
  - 1080p60 target where the phone advertises it; sustained 30 fps minimum; no positive frame-age slope; total latency under 100 ms for the accepted transport/test setup; bounded memory; reconnect time and A/V offset thresholds.
- [ ] **Step 2: Run USB, clean Wi-Fi, congested Wi-Fi, dim/background, camera-switch, and thermal-soak scenarios**
  - Record percentiles rather than only averages.
- [ ] **Step 3: Fix only regressions exposed by the gates**
  - Use a failing regression test or repeatable benchmark before each fix.
- [ ] **Step 4: Update README/protocol claims from measured behavior**
  - Remove unsupported 4K60/~30 ms claims.
- [ ] **Step 5: Commit release validation documentation**
  - Commit message: `docs: add CamDroid performance and release gates`.

## Self-review

- Spec coverage: media timing, queue bounds, negotiation, sender nonblocking behavior, receiver conversion/reuse, recovery, Android capability/state/UI, physical testing, desktop state, and release gates each have an owning task.
- Interface consistency: Task 1 produces packet vectors consumed by Tasks 2–6; Task 2 produces `NegotiatedStream` consumed by Tasks 3–7; Task 4 produces queues consumed by Task 5; Task 7 produces `StreamUiState` consumed by Task 8 and capability/session state consumed by Task 10.
- The plan deliberately defers Android compilation and physical validation until Task 9, matching the user’s request. Source-level tests can be written before then; they are not claimed to have run.
- The current plan does not promise that the OPPO can physically capture 1080p60. It requires a measured capability result and a truthful fallback.
