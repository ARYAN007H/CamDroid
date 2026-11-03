# CamDroid: handoff

Branch: `testing` (not yet merged to `main`).
Last push: `4d1f6e8`. Everything below this line that is listed as committed
is on the branch; the "uncommitted" section is in the final commit of this push.

## What's done

### Desktop core (Rust, `desktop/`)
- **Video gate** (`media_queue.rs`): inter-coded video only reaches the decoder
  while its reference chain is intact. Sequence gaps, discontinuities and
  session changes drop to the next key frame and request one from the phone.
- **Decoder** (`video/decoder.rs`): decodes every admitted packet in order,
  converts only the newest picture per batch. Repeated errors request a key frame.
- **Render stage** (`video/render.rs`): newest-frame selection, live preview
  (~30 fps, downscaled), optional v4l2loopback output switchable at runtime,
  backoff retry when the device is missing, horizontal mirror.
- **Looks**: colour grade as a tested 3x256 LUT (bit-identical to the reference
  path); spatial effects for Pixel, VHS, Neon Edge, Cartoon (`video/effects.rs`).
- **Negotiation** (`session.rs`): the desktop asks for a mode and the phone's
  advertised modes decide the closest match, with a bitrate recommendation.
- **Telemetry** (`telemetry.rs`): fps, drops, decode errors, battery, virtual
  camera state, disconnect reason (first reason kept).
- **Connection health**: 10 s liveness timeout; the session reports why it ended.
- **Discovery**: deadline-bounded, IPv4 only, deduplicated.
- **CLI**: mirror toggle fixed (it always sent `enabled: true`).

Tests: 97 unit tests + 7 end-to-end tests. The end-to-end tests run a fake phone
over real TCP with a real H.264 clip (`desktop/tests/end_to_end.rs`,
`tests/fixtures/`) and cover: clean playback red→blue, adapting to the phone's
only mode, lost-packet recovery at the next key frame, phone hang-up detection,
stop semantics, and connecting to a closed port.

### Tauri backend (`desktop-tauri/src-tauri/`)
- Commands: discover, connect (off the UI thread), session status, disconnect,
  start preview (JPEG over a binary channel), camera control (validated),
  filters, mirror, output mode, ring light, presets, system check, quit.
- Persisted settings: tolerant load, atomic write, coalesced background saves.
- Ring-light overlay window: transparent, always on top, click-through.
- Borderless main window (`decorations: false`), least-privilege capabilities,
  strict CSP, real app icons generated from a flat design.
- 28 unit tests pass. Clippy clean.

### Frontend (`desktop-tauri/src/`)
- Full rewrite. Flat graphite design, no glass/blur, single accent.
- Connect screen: find phone / enter address / USB, stream quality, readiness hints.
- Studio: full-bleed viewfinder, auto-hiding HUD and dock, looks tray (1-9 keys),
  adjust panel (look sliders, zoom, focus, exposure, white balance, flash,
  output, ring light, stream stats), connection-lost overlay, shortcut overlay (?).
- Typed API layer with a browser mock (`npm run dev:mock`, `#lost` in the URL
  simulates a dropped connection).
- 19 frontend unit tests pass; typecheck clean.
- Screenshots were taken in WebKitGTK (the engine Tauri uses on Linux) and the
  connect, studio, looks tray, adjust panel and shortcut overlay all rendered.

### Repo hygiene
- `node_modules` and build logs untracked; root `.gitignore` added.

## What's left (in order)

1. **Run the real Tauri build** on your machine: `cd desktop-tauri && npm ci && npm run app:bundle`
   (needs the WebKitGTK/GTK dev packages listed in `desktop-tauri/src-tauri`).
   Check the borderless window, drag-to-move, edge resize, and the ring light on your desktop.
   Browser screenshots cannot show window behaviour.
2. **Visual pass on the rest of the studio** with the mock: the disconnect overlay
   (`dev:mock` then open `#lost`), the stream-quality expander, and the readiness
   line when the virtual camera is missing.
3. **Accessibility/keyboard pass**: tab order through the connect screen and the
   adjust panel; focus rings on the dock.
4. **Android side**: the desktop already expects protocol v2 with negotiation,
   but the Android build has not been verified. It needs JDK 17 (the build logs
   failed on JDK 25/26 under Kotlin 2.0.21). Pin the Gradle JDK, then run
   `./gradlew testDebugUnitTest` and `installDebug`.
5. **Real-device test** with the OPPO A5x over USB: confirm the negotiated mode,
   key-frame recovery on real packet loss, and virtual camera in OBS/Zoom.
6. **Merge `testing` into `main`** once 1 and 5 pass. `main` has not been touched.

## Known gaps / notes

- Mirror and zoom are applied on the receiver for mirror, and on the phone for zoom.
  Phone-side mirror is not implemented (`set_mirror` on Android only stores the flag).
- Spatial effects run on the CPU per frame. They are cheap at preview size but
  unbenchmarked at 1080p60 on the i3-1115G4. Benchmark before enabling on the
  virtual camera path by default.
- The preview is JPEG-encoded per frame; at 30 fps this is fine, but it is the
  next place to look if CPU use is high.
- `ffmpeg-next` 9.0 with FFmpeg 6.1 headers builds here; FFmpeg 7+ is untested.
- The fixture clip is generated with the command in `end_to_end.rs`; regenerate
  it if the test expectations change.

## How to pick this up

```
git checkout testing && git pull
cd desktop && cargo test            # 97 unit + 7 end-to-end
cd ../desktop-tauri && npm ci && npx vitest run && npm run build
```
