# Desktop Camera Studio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a polished CamDroid desktop camera studio with shared standalone/OBS output, GPU-preferred filtering, and a macOS-style virtual ring-light overlay.

**Architecture:** The Rust media core owns output/filter state and applies a bounded, deterministic filter stage before V4L2. Tauri exposes that state through typed commands to a redesigned studio. A separate transparent Tauri window draws the ring-light overlay and persists its validated settings.

**Tech Stack:** Rust, FFmpeg, V4L2, Tauri 2, TypeScript, Vite, CSS, WebGL-capable WebView, Intel Vulkan/CPU fallback.

**Spec:** `docs/superpowers/specs/2026-10-25-desktop-camera-studio-design.md`

## Global Constraints

- Linux/V4L2 is the first target; OBS uses the named CamDroid virtual camera as a standard Video Capture Device.
- Prefer an available hardware renderer, including Intel integrated graphics; never require a discrete GPU.
- Preserve bounded latest-frame delivery and never block capture/decoder callbacks for rendering.
- CPU fallback must be feature-correct and status-visible.
- Ring Light must use a transparent, always-on-top overlay with a central rounded opening; if the compositor rejects transparency, display a truthful unsupported-state message.
- Do not claim OBS scene automation or an OBS plugin.

## Review Focus

- Unsupported hardware renderer: test that backend selection falls back to CPU with a user-visible reason in Task 1.
- Slow filter/output: test that only newest frames reach the effect stage in Task 2.
- Invalid filter/ring-light payloads from the frontend: test strict enum and range validation in Task 3.
- Missing/unwritable V4L2 device: test that OBS mode reports unavailable rather than connected in Task 3.
- Transparent window unavailable under a compositor: test/show a recoverable overlay error in Task 4.

### Task 1: Add filter state and a GPU-preferred renderer contract

**Files:**
- Create: `desktop/src/video/filters.rs`, `desktop/src/video/filters_tests.rs`
- Modify: `desktop/src/video/mod.rs`, `desktop/src/lib.rs`

**Interfaces:**
- Produces: `FilterPreset`, `FilterSettings`, `RenderBackend`, `RendererStatus`, `FilterController`.
- Consumed by: Task 2 V4L2 output and Task 3 Tauri commands.

- [ ] **Step 1: Write failing pure-Rust tests**
  - Assert valid presets map to a deterministic setting, invalid numeric settings are clamped/rejected, colour transforms alter a known YUYV sample predictably, and unavailable GPU selection becomes `RenderBackend::CpuFallback` with a reason.
- [ ] **Step 2: Run the focused test to verify RED**
  - Run: `cargo test --manifest-path desktop/Cargo.toml filters`
  - Expected: module/type errors.
- [ ] **Step 3: Implement filter state and CPU reference transforms**
  - Define `FilterPreset::{Natural,WarmFilm,TealOrange,Noir,Vintage,Pixel,Vhs,NeonEdge,Cartoon}` and a shared `FilterController` using `Arc<RwLock<FilterSettings>>`.
  - Define `RendererStatus { backend, available, detail }`; select an available GPU-capable path when compiled/available, otherwise return the CPU fallback status without changing the selected look.
- [ ] **Step 4: Run focused and full core tests**
  - Run: `cargo test --manifest-path desktop/Cargo.toml filters` then `cargo test --manifest-path desktop/Cargo.toml`
  - Expected: all pass.
- [ ] **Step 5: Commit**
  - Message: `feat: add desktop filter state and renderer fallback`.

### Task 2: Apply selected filters to the virtual-camera output

**Files:**
- Modify: `desktop/src/video/decoder.rs`, `desktop/src/video/v4l2_output.rs`, `desktop/src/lib.rs`
- Test: `desktop/src/video/filters_tests.rs`

**Interfaces:**
- Consumes: Task 1 `FilterController` and existing bounded frame channels.
- Produces: `spawn_v4l2_writer_thread(..., FilterController, ...)` and output statistics that identify the active backend/preset.

- [ ] **Step 1: Write failing output-stage tests**
  - Assert a selected preset reaches the output transform, `Natural` leaves a fixture unchanged, and stale frames are discarded before transform work.
- [ ] **Step 2: Run focused tests to verify RED**
  - Run: `cargo test --manifest-path desktop/Cargo.toml video::filters`
  - Expected: missing output/filter integration.
- [ ] **Step 3: Add the shared controller to V4L2 output**
  - Process only the freshest eligible frame, apply the active transform immediately before `write_all`, and retain measured output/drop timings without adding a queue.
- [ ] **Step 4: Run core tests**
  - Run: `cargo test --manifest-path desktop/Cargo.toml`
  - Expected: all pass.
- [ ] **Step 5: Commit**
  - Message: `feat: filter CamDroid virtual camera output`.

### Task 3: Expose truthful desktop output and filter controls

**Files:**
- Modify: `desktop-tauri/src-tauri/src/commands.rs`, `desktop-tauri/src-tauri/src/lib.rs`
- Create: `desktop-tauri/src-tauri/src/studio.rs`, `desktop-tauri/src-tauri/src/studio_tests.rs`

**Interfaces:**
- Consumes: Task 1 `FilterController`, renderer/output state, session configuration.
- Produces: typed `StudioState`, `OutputMode`, `RingLightSettings`, `get_studio_state`, `set_output_mode`, `set_filter`, `set_ring_light_settings` commands.

- [ ] **Step 1: Write failing state tests**
  - Assert OBS mode rejects unavailable virtual-camera output, standalone mode does not claim OBS readiness, invalid presets/settings are rejected, and a valid ring-light state is normalized to documented ranges.
- [ ] **Step 2: Run the test to verify RED**
  - Run: `cargo test --manifest-path desktop-tauri/src-tauri/Cargo.toml studio`
  - Expected: module/type errors.
- [ ] **Step 3: Implement typed studio state and commands**
  - Do not hold the session mutex during any blocking check. Detect the V4L2 path read-only and expose its exact readiness/error.
- [ ] **Step 4: Run Rust checks**
  - Run: `cargo test --manifest-path desktop-tauri/src-tauri/Cargo.toml`
  - Expected: all pass.
- [ ] **Step 5: Commit**
  - Message: `feat: expose desktop studio output controls`.

### Task 4: Build the standalone/OBS studio and Ring Light overlay

**Files:**
- Create: `desktop-tauri/src/pages/studio.ts`, `desktop-tauri/src/pages/ring-light.ts`, `desktop-tauri/ring-light.html`
- Modify: `desktop-tauri/src/main.ts`, `desktop-tauri/src/style.css`, `desktop-tauri/src-tauri/tauri.conf.json`, `desktop-tauri/package.json`
- Test: frontend type/build command.

**Interfaces:**
- Consumes: Task 3 typed commands and state.
- Produces: two-mode studio UI, filter inspector, renderer/OBS readiness rail, accessible ring-light overlay controls.

- [ ] **Step 1: Write frontend state tests or pure controller tests**
  - Assert mode/status labels derive from `StudioState`, only one preset is selected, and ring-light CSS variables are generated from normalized settings.
- [ ] **Step 2: Run frontend type check to verify RED**
  - Run: `npm run build --prefix desktop-tauri`
  - Expected: missing studio/overlay imports.
- [ ] **Step 3: Implement the clean studio workspace**
  - Replace the placeholder dashboard with a preview surface, Standalone/OBS segmented control, explicit virtual-camera device/status, cinematic and fun filter groups, camera controls, and keyboard-visible focus states.
- [ ] **Step 4: Implement ring-light overlay**
  - Open one named transparent, always-on-top Tauri window using the overlay page. Render a soft white/warm rounded-rectangle mask with a transparent central opening; close safely and report unsupported transparency through state.
- [ ] **Step 5: Run frontend build**
  - Run: `npm run build --prefix desktop-tauri`
  - Expected: success.
- [ ] **Step 6: Commit**
  - Message: `feat: build CamDroid desktop camera studio`.

### Task 5: Verify the desktop application and physical stream path

**Files:**
- Create: `docs/validation/desktop-camera-studio.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: Tasks 1–4 and connected OPPO device.
- Produces: measured desktop/OBS/ring-light validation record and accurate setup instructions.

- [ ] **Step 1: Run all desktop verification**
  - Run: `cargo test --manifest-path desktop/Cargo.toml`, `cargo test --manifest-path desktop-tauri/src-tauri/Cargo.toml`, `npm run build --prefix desktop-tauri`.
  - Expected: all pass.
- [ ] **Step 2: Test USB streaming on the OPPO**
  - Connect through the desktop app, verify V4L2 readiness, select a cinematic filter then a fun filter, and record stream/error observations.
- [ ] **Step 3: Test OBS mode and Ring Light**
  - Launch OBS, select CamDroid as Video Capture Device, verify it shows filtered output; toggle Ring Light and record compositor behavior. Report any missing driver/compositor capability as a limitation, not success.
- [ ] **Step 4: Document only measured results**
  - Add setup, device name, test outcomes, and exact limitations to the validation doc and README.
- [ ] **Step 5: Commit**
  - Message: `docs: validate desktop camera studio`.

## Self-review

- Spec coverage: standalone studio, OBS mode, virtual-camera truthfulness, GPU-preferred/CPU fallback rendering, shared filter output, low-latency constraints, fun effects, and Apple-style ring light each have an owning task.
- Type consistency: Task 1 defines filter/backend interfaces consumed by Tasks 2–3; Task 3 defines studio/overlay state consumed by Task 4; Task 5 validates the assembled product.
- Review focus coverage: renderer fallback (Task 1), stale-filter work (Task 2), invalid IPC and V4L2 readiness (Task 3), overlay compositor failure (Task 4), and physical OBS behavior (Task 5) are explicitly tested.
