# CamDroid Desktop Camera Studio Design

## Intent

Turn the existing CamDroid Tauri shell into a usable desktop camera studio. It must work as a standalone companion for connecting and controlling the phone, and as an OBS-ready virtual camera. The same visual treatment must reach the virtual camera so OBS receives the selected look without a second configuration step.

## Goals

- Present a clean, responsive desktop studio with connection, live output health, camera controls, colour looks, fun effects, and OBS readiness.
- Provide GPU-preferred rendering that works on an Intel integrated GPU and has a safe CPU fallback.
- Apply the selected video look before CamDroid writes to its V4L2 virtual camera; OBS users select the CamDroid camera as a normal video device.
- Offer two deliberate modes: Standalone Camera and OBS Mode. OBS Mode keeps the virtual camera output active and makes its state obvious.
- Add an Apple-style virtual ring light: an always-on-top, soft white rounded-rectangle overlay with a large transparent centre, warm/cool control, brightness, thickness, and a one-click close affordance.

## Non-goals

- Do not claim a native OBS plugin or automatic OBS scene/source creation.
- Do not require a dedicated GPU, an emulator, a cloud service, or a new Android feature.
- Do not make the ring-light overlay click-through until that behavior is verified on the running Linux compositor; an unreliable click-through overlay would make the desktop unusable.

## Approaches considered

1. **GPU-preferred native pipeline (chosen).** A renderer abstraction probes a hardware backend first and exposes a documented CPU fallback. The selected filter runs before the virtual-camera writer. This preserves a single visual result for the studio and OBS, with bounded queues protecting latency.
2. **Frontend-only WebGL filters.** This makes an attractive preview but cannot reliably feed processed frames into the existing Rust/V4L2 output path, so OBS would receive unfiltered video.
3. **OBS-only filters.** This is inexpensive but makes the desktop app a remote control rather than a standalone camera studio.

## Interaction design

- The studio opens to a calm dark workspace: output mode in the header, large preview surface, an output/OBS status rail, and a right-side inspector.
- The inspector separates **Cinematic colour** (Natural, Warm Film, Teal & Orange, Noir, Vintage) from **Fun effects** (Pixel, VHS, Neon Edge, Cartoon). Only one filter can be active. Heavy effects carry a small performance note.
- “OBS Mode” is a mode switch, not a second app. When active, the app starts/retains the virtual camera output and displays the exact device name and readiness result. OBS consumption remains standard Video Capture Device selection.
- Ring Light opens a dedicated overlay window. It fills the display with a non-interactive-looking luminous mask, has a generous central rounded opening, and shows minimal controls only when the overlay is focused. It remembers the last brightness, colour temperature, and thickness locally.

## Architecture

- Add a pure Rust `video::filters` module. It defines `FilterPreset`, `RenderBackend`, `FilterSettings`, and deterministic RGBA/YUYV-safe pixel transforms for the CPU fallback. The core session owns the selected setting via thread-safe shared state.
- Add a GPU-preferred renderer selection module. It selects a usable hardware backend when the build/runtime exposes one, records the selected backend in output status, and always falls back to the tested CPU filter path. The renderer contract is designed so a zero-copy backend can replace the CPU implementation without changing Tauri commands or filter semantics.
- The V4L2 output stage consumes the selected `FilterSettings`, processes the freshest decoded frame, then writes to the existing CamDroid virtual camera. This means standalone and OBS outputs agree.
- Tauri exposes typed commands for output mode, filter selection, ring-light state, and output status. The frontend uses these commands and never invents readiness/telemetry.
- A secondary Tauri overlay window hosts the ring light. If compositor transparency is unavailable, it reports that limitation and does not obscure the user’s screen.

## Constraints

- Linux/V4L2 is the first target. The virtual-camera device must be detected and displayed truthfully; OBS discovery is not assumed.
- Filtering must not add unbounded queues or block capture/decoder callbacks.
- CPU fallback must keep the app functional on the Intel i3-1115G4; CPU-intensive effects may be disabled or reported as degraded based on measured frame time.
- Ring-light settings must be accessible by keyboard and recoverable after application restart.

## Verification

- Unit tests cover preset mapping, deterministic colour transforms, backend fallback, output-mode state transitions, and ring-light state validation.
- Frontend type/build checks cover the studio, output mode, and overlay assets.
- A physical-device run validates connection, virtual-camera readiness, active filter state, OBS-mode behavior, and the ring-light overlay on the user’s desktop. Actual OBS ingestion is verified only after OBS is opened and selects the CamDroid device.
