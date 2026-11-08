# Codex Progress Log: CamDroid Rebuild & Desktop Camera Studio

**Branch:** `testing`  
**Date:** 2026-10-25  
**Target Hardware:**  
- Phone: OPPO A5x 5G  
- Desktop: Intel Core i3-1115G4 (integrated graphics), 8GB DDR4 RAM, Linux (Arch Linux)

---

## 1. What Codex Was Trying to Do

### Primary Goal
Rebuild CamDroid's end-to-end media and session architecture from the ground up to achieve:
- **True ultra-low latency (<100ms)** instead of seconds of buffer buildup.
- **Sustained 1080p @ 30–60 FPS** on hardware-constrained systems (Intel i3 with no dedicated GPU).
- **A Desktop Camera Studio**: A modern Tauri 2 desktop companion supporting:
  - **Standalone Mode & OBS Mode** (direct output to `/dev/video10` via `v4l2loopback`).
  - **Cinematic Color Grades & Fun Effects**: GPU-preferred rendering with a CPU fallback for Intel integrated graphics (*Warm Film, Teal & Orange, Noir, Vintage, Pixel, VHS, Neon Edge, Cartoon*).
  - **Virtual Ring Light**: An Apple-style soft, luminous always-on-top overlay with transparent center and color temperature controls.

### Technical Plans & Specs Created by Codex
1. [`docs/AUDIT.md`](./AUDIT.md): Comprehensive audit identifying latency traps (120-frame FIFO buffers, lack of PTS/media timestamps, thread-blocking socket writes, excessive YUYV frame copies).
2. [`docs/superpowers/plans/2026-10-16-camdroid-rebuild-plan.md`](./superpowers/plans/2026-10-16-camdroid-rebuild-plan.md): Multi-phase rebuild plan covering Protocol V2, non-blocking Android send queues, negotiated session state, and bounded latest-frame desktop queues.
3. [`docs/superpowers/specs/2026-10-25-desktop-camera-studio-design.md`](./superpowers/specs/2026-10-25-desktop-camera-studio-design.md): Design spec for the Desktop Camera Studio with OBS virtual camera integration, video filter pipeline, and macOS-style ring light.
4. [`docs/superpowers/plans/2026-10-25-desktop-camera-studio-plan.md`](./superpowers/plans/2026-10-25-desktop-camera-studio-plan.md): 5-task execution plan for the studio and virtual camera output.

---

## 2. Completed Work (Committed)

The following milestone commits were completed and verified on this branch:

1. **Timestamped Media Protocol V2 (`da084ba`)**:
   - Upgraded `ffmpeg-next` from 8.1 to 9.0 to resolve Arch Linux / FFmpeg 7 header compatibility.
   - Defined binary Protocol V2 in Rust (`desktop/src/protocol/v2.rs`) and Kotlin (`android/.../ProtocolV2.kt`).
   - Added 64-bit nanosecond capture timestamps, sequence numbers, session IDs, and payload safety limits.

2. **Negotiated Streaming Session State (`3183724`)**:
   - Implemented bidirectional capability exchange and handshake (`desktop/src/session.rs` and `android/.../SessionState.kt`).
   - Enforced explicit state transitions: `Idle` → `Connecting` → `Negotiating` → `Streaming`.

3. **Decoupled Android Media Sender (`b5cc060` & `da65b72`)**:
   - Introduced `MediaSender.kt` with non-blocking bounded coroutine channels.
   - Decoupled `MediaCodec` hardware encoder callbacks from TCP socket I/O to prevent frame-drop stutter on Android.

4. **Desktop Video & Audio Bounded Queues (`b504aa3`)**:
   - Created `desktop/src/media_queue.rs` with latest-frame retention policy.
   - Automatically drops stale frames when queues fill, preserving sub-100ms real-time latency.
   - Preserved capture and presentation timestamps through the demuxing and decoding pipeline.

5. **Video Filter Engine & Renderer Fallback (`7d2feb7`)**:
   - Created `desktop/src/video/filters.rs` defining `FilterPreset`, `FilterSettings`, and deterministic color transforms.
   - Integrated renderer fallback tracking (`RenderBackend::CpuFallback` vs GPU-accelerated).
   - Added unit tests for filter deterministic color transformations.

---

## 3. Work In-Progress / Uncompleted

### Uncommitted / Work-in-Progress Changes Captured in this Snapshot:
- **`desktop/src/video/v4l2_output.rs`**:
  - Implemented `take_newest_frame()` to drain queued frames and discard stale ones before effect processing.
  - Implemented `prepare_frame_for_output()` applying active `FilterController` transforms prior to writing to the V4L2 device.
  - Added unit tests: `newest_frame_is_selected_before_filter_work`, `output_transform_applies_the_selected_preset`, and `output_transform_leaves_natural_fixture_unchanged`.
- **`desktop/src/audio/pulse_output.rs`, `desktop/src/connection/adb.rs`, `desktop/src/connection/discovery.rs`, `desktop/src/control/remote.rs`, `desktop/src/lib.rs`**:
  - Cleanup, formatting, and integration touchpoints for the new session and filter controllers.

### Tasks Remaining in the Plan:
- **Task 3: Desktop Studio Tauri IPC Backend**:
  - Create `desktop-tauri/src-tauri/src/studio.rs` with typed commands for `get_studio_state`, `set_output_mode`, `set_filter`, and `set_ring_light_settings`.
- **Task 4: Desktop Studio Frontend & Ring Light Overlay**:
  - Implement two-mode studio UI (`desktop-tauri/src/pages/studio.ts`).
  - Implement transparent always-on-top ring light window (`desktop-tauri/src/pages/ring-light.ts` & `ring-light.html`).
- **Task 5: End-to-End Device Verification**:
  - Physical end-to-end testing with OPPO A5x 5G connected over USB/ADB and V4L2 loopback into OBS Studio.

---

## 4. Current Test Status
- **Desktop Core Unit Tests**: 33 tests passed, 0 failed (`cargo test --manifest-path desktop/Cargo.toml`).
