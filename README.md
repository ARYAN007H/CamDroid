<div align="center">

<img src="https://raw.githubusercontent.com/google/material-design-icons/master/png/device/videocam/materialiconssharp/240dp/2x/sharp_videocam_black_24dp.png" width="110" alt="CamDroid">

# 🎥 CamDroid

### Your Android phone, reborn as a proper Linux webcam.

**Hardware-encoded video. Real-time controls. Zero watermarks. A whole studio.**

[![Release](https://img.shields.io/github/v/release/ARYAN007H/CamDroid?style=flat-square&color=24C8D8&label=release)](https://github.com/ARYAN007H/CamDroid/releases/latest)
[![Release CI](https://img.shields.io/github/actions/workflow/status/ARYAN007H/CamDroid/release.yml?branch=main&style=flat-square&label=release%20CI)](https://github.com/ARYAN007H/CamDroid/actions/workflows/release.yml)
![Platform](https://img.shields.io/badge/platform-Android%20%2B%20Linux-purple?style=flat-square)
![Protocol](https://img.shields.io/badge/protocol-v2%20%22CMD2%22-orange?style=flat-square)
[![License](https://img.shields.io/github/license/ARYAN007H/CamDroid?style=flat-square&color=green)](LICENSE)

</div>

---

> 📱 **A phone in your pocket is a better camera than any webcam ever made.**
> CamDroid streams its hardware-encoded H.264/H.265 + AAC over a purpose-built
> binary protocol to your Linux desktop — and *CamDroid Studio* turns the whole
> thing into a control room: looks, rotation, aspect crop, a ring light, live
> telemetry, and one-click virtual-camera output for OBS, Zoom and Chrome.

## 🧩 Three apps, one pipeline

| | App | What it is |
| :---: | :--- | :--- |
| 📱 | **CamDroid** (Android) | The transmitter — a foreground service that opens the camera, hardware-encodes, and speaks protocol v2 on port `4747` |
| 🎛️ | **CamDroid Studio** (Linux GUI) | The control room — discovery, live stage, filters, rotation & crop, ring light, phone camera controls, virtual webcam |
| 🎮 | **camdroid-client** (Linux CLI) | The purist path — the same pipeline in your terminal with an interactive `camdroid>` console |

<details>
<summary><b>⚔️ How it stacks up</b></summary>

| Feature | **CamDroid 📸** | DroidCam (Free) | DroidCam (Premium) | VDO.ninja |
| :--- | :---: | :---: | :---: | :---: |
| Max resolution | 💎 **4K (2160p)** | 480p | 1080p | 1080p |
| Max frame rate | ⚡ **60 FPS**\* | 30 FPS | 30 FPS | 30 / 60 FPS |
| Video codecs | **H.264 / H.265 / MJPEG** | H.264 | H.264 | VP8 / VP9 / H.264 |
| Audio | 🎙️ **AAC-LC (hardware)** | Low-Q mono | Standard mono | Opus |
| Latency | 🏎️ **~30–50 ms** | ~70–120 ms | ~70–120 ms | ~150–300 ms |
| Desktop controls | 🎛️ **Studio + CLI** | No | Web UI only | No |
| Packet-loss policy | 🩹 **Shed + resync + keyframe** | Stutter | Stutter | WebRTC adaptive |
| Open source | 🔓 **MIT** | No (ads) | No (paid) | Yes |

\* *wherever the phone's camera hardware honestly allows it — see
[Frame-rate honesty](#-frame-rate-honesty-why-not-always-60).*

</details>

---

## 🗺️ How a frame gets from your pocket to OBS

```mermaid
flowchart TD
    classDef phone fill:#5d3fd3,stroke:#fff,stroke-width:2px,color:#fff;
    classDef pc fill:#00E5FF,stroke:#fff,stroke-width:2px,color:#000;
    classDef net fill:#333,stroke:#ccc,stroke-width:1px,color:#fff;

    subgraph Phone ["📱 Android transmitter"]
        Camera[Camera2 API] -->|YUV / OES| VEnc[MediaCodec H.264 · H.265 · MJPEG]
        Mic[AudioRecord] -->|PCM| AEnc[MediaCodec AAC-LC]
        VEnc --> Sender[MediaSender<br/>per-kind sequences · shedding]
        AEnc --> Sender
        Sender -->|CMD2 packets| Server[TCP server :4747]
        NSD[NsdManager] -.->_camdroid._tcp.-> mDNS[mDNS]
    end

    Server -->|WiFi · ADB forward| Core["🦀 camdroid-client core<br/>video gate · demux · ABR"]

    subgraph Desktop ["💻 Linux desktop"]
        Core --> VDec[FFmpeg video decode] --> V4L2[v4l2loopback /dev/video10]
        Core --> ADec[FFmpeg AAC decode] --> Pulse[PulseAudio / PipeWire source]
        Core --> Studio["🎛️ CamDroid Studio"]
        Core --> CLI[Interactive CLI]
    end

    Studio -->|camera commands| Core
    CLI -->|control commands| Core
    Core -->|negotiation + controls| Server
    V4L2 --> Apps[OBS · Zoom · Discord · Browsers]
    Pulse --> Apps

    class Camera,VEnc,Mic,AEnc,Sender,Server,NSD phone;
    class Core,VDec,ADec,V4L2,Pulse,Studio,CLI,Apps pc;
    class mDNS net;
```

<details>
<summary><b>📦 Protocol v2 — every packet is framed, sequenced and timestamped</b></summary>

```
 ┌──────────┬───┬────┬───────┬────────────┬──────────┬───────────────────┬────────┬─────────┐
 │ "CMD2"   │ v │kind│ flags │ session_id │ sequence │ capture_ts_ns     │ length │ payload │
 │ 4 bytes  │ 1 │ 1  │ 2     │ 8 bytes    │ 8 bytes  │ 8 bytes           │ 4 bytes│ …       │
 └──────────┴───┴────┴───────┴────────────┴──────────┴───────────────────┴────────┴─────────┘
   little-endian · kinds: video · audio · control · metadata · heartbeat · codec configs
   flags: key frame · configuration unit · discontinuity (resync hint)
```

- **Per-kind sequence spaces** — audio interleaving can never poke holes in the video
  sequence, so the receiver's gate admits *every* frame in order.
- **Sender-side shedding** — when the link falls behind, the phone drops the frames
  nobody will miss and stamps the next one with a discontinuity + fresh key frame.
  The picture recovers within one GOP instead of freezing.
- **Capability negotiation** — the phone advertises what its camera *actually* supports;
  the desktop resolves its wish against that truth. No "requested mode unavailable" surprises.
- Full spec: [`protocol/PROTOCOL.md`](protocol/PROTOCOL.md)

</details>

---

## ✨ CamDroid Studio

<div align="center">

`Discover → Connect → You're live.`

</div>

- 🖼️ **A real stage** — idle-hiding overlays like a video player, connection-loss handling, live badge and telemetry
- 🔄 **Rotate 90° left / right** (`Z` / `X`) and **aspect-ratio crop** — Source, 16:9, 4:3, 1:1, 9:16 — persisted between runs
- 🪞 **Mirror**, colour-**looks** (natural, warm, vivid, mono, VHS, neon edge, cartoon…) with intensity/temperature/saturation/contrast
- 💡 **Ring light** — a soft glowing screen border that lights your face on dark calls, with brightness, warmth and thickness
- 🎥 **Virtual camera** — appear as `/dev/video10` in OBS, Zoom, Teams and browsers with one toggle
- 📞 **Remote phone controls** — zoom, auto/manual focus, exposure, white balance, flash, front/rear switch, resolution, fps, codec, bitrate
- 📊 **Live telemetry** — negotiated mode, render fps, dropped frames, phone battery

<details>
<summary><b>⌨️ Keyboard shortcuts</b></summary>

| Key | Action | | Key | Action |
| :---: | :--- | --- | :---: | :--- |
| `E` | Looks tray | | `+` / `-` | Zoom in / out |
| `I` | Adjust panel | | `0` | Reset zoom |
| `M` | Mirror | | `C` | Switch front / back camera |
| `Z` / `X` | Rotate left / right | | `]` / `[` | Next / previous look |
| `R` | Ring light | | `1–9` | Pick a look |
| `O` | Virtual camera | | `F` / `F11` | Full screen |
| `Ctrl+D` | Disconnect | | `?` | All shortcuts |
| `Ctrl+Q` / `Ctrl+W` | Quit | | `Esc` | Close panel |

</details>

---

## ⚖️ Frame-rate honesty (why not always 60?)

CamDroid never advertises a frame rate the camera can't deliver. Capabilities are read
from the HAL — `CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES` plus the constrained high-speed
configuration map. If the device exposes a **fixed** `[60,60]` high-speed mode for the
chosen resolution, CamDroid opens a proper `SESSION_HIGH_SPEED` session with a batched
request list and streams 60.

Many OEM pipelines (e.g. OPPO/realme's `cameraUnit` vendor session) hand their >30 fps
modes only to their own camera app — third-party apps get `"authentication fails"`.
On those phones CamDroid reports and **locks a rock-steady 30** instead of pretending.
No fake badges, no stutter, no lies.

---

## 🚀 Quick start

### 0 · Prerequisites

| You need | For | Notes |
| :--- | :--- | :--- |
| Linux + `v4l2loopback` | virtual camera | `desktop/setup.sh` loads and pins it at boot |
| FFmpeg 9 dev libs | desktop core | preinstalled on Arch; see the CI container for a canonical recipe |
| Node.js + Rust | Studio build | `@tauri-apps/api` minor must match the Rust crate (pinned) |
| JDK 17 + Android SDK | Android app | Kotlin 2.0 rejects JDK 25 toolchains |

### 1 · Android transmitter

```bash
cd android
JAVA_HOME=/usr/lib/jvm/java-17-openjdk ./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```
Open the app, tap the shutter — the server is live on `:4747`.

### 2a · CamDroid Studio (recommended)

```bash
cd desktop-tauri
npm install
npm run tauri build        # bundles land in src-tauri/target/release/bundle/
```

### 2b · CLI client

```bash
cd desktop
cargo build --release
```

### 3 · Connect

| Mode | Studio | CLI |
| :--- | :--- | :--- |
| **USB (ADB)** — lowest latency ⚡ | pick the device under *USB (ADB)* | `camdroid-client --usb` |
| **WiFi (auto-discovery)** | pick it under *WiFi* | `camdroid-client --discover` |
| **WiFi (direct IP)** | *WiFi (Direct IP)* → `ip:4747` | `camdroid-client --connect 192.168.1.67:4747` |

> [!TIP]
> Whatever resolution/fps/codec you pick, the desktop resolves it against the phone's
> advertised capability — on a phone that can't do 60, your request lands on 30
> automatically instead of failing.

<details>
<summary><b>🎮 CLI remote-control console</b></summary>

```text
camdroid> help
  zoom <value> · focus auto|manual <dist> · exposure <ev> · wb <mode>
  flash on|off · mirror · camera
  resolution <res> · fps <value> · codec <c> · bitrate <bps> · keyframe
  status · help · quit
```

Macroblocking on a congested network? `keyframe` forces a fresh IDR frame and
the picture snaps clean. (Studio's sender-side shedding usually beats you to it.)

</details>

---

## 🤖 Automated releases

Pushing a tag runs the [release pipeline](.github/workflows/release.yml):

```
tag v*  ──►  📱 Android APK (ubuntu)      ──┐
         └─►  🎛️ deb + rpm (Arch container) ─┴─►  GitHub release with artifacts
```

The desktop job builds inside an **Arch Linux container** so the FFmpeg 9 +
webkit2gtk stack matches the environment the releases are verified on — Ubuntu's
older FFmpeg would break the core. Manual dispatches run the build jobs alone
as a smoke test.

---

## 🛠️ Troubleshooting

<details>
<summary><b>Virtual camera device /dev/video10 not found</b></summary>

```bash
lsmod | grep v4l2loopback        # is it loaded?
sudo modprobe v4l2loopback exclusive_caps=1 video_nr=10 card_label="CamDroid"
```
`exclusive_caps=1` is required by Chrome and Zoom.
</details>

<details>
<summary><b>Virtual microphone missing</b></summary>

With PipeWire, make sure the PulseAudio compatibility layer is running:
`systemctl --user status pipewire-pulse`. Distorted audio usually means the
phone-side microphone permission is missing.
</details>

<details>
<summary><b>USB connection fails</b></summary>

USB Debugging must be on and authorized (`adb devices` must not show
`unauthorized`). A stale forward blocks port 4747:
`adb forward --remove tcp:4747`, then reconnect.
</details>

<details>
<summary><b>"Requested video mode is unavailable"</b></summary>

The phone advertises only what its camera truly supports. The desktop resolves
your request automatically; if you still hit this, pick a lower resolution or
30 fps in the connect screen.
</details>

<details>
<summary><b>Locked at 30 FPS</b></summary>

See <a href="#-frame-rate-honesty-why-not-always-60">Frame-rate honesty</a> —
the OEM gates &gt;30 fps modes behind its own camera app on some devices.
</details>

---

## 🤝 Contributing

Issues and PRs welcome — the codebase is small on purpose: a Kotlin app, a Rust
core, and a Tauri UI. `cargo test` (core), `npm test` (frontend) and
`./gradlew testDebugUnitTest` (Android) should all stay green.

<div align="center">

**Built for people whose webcam is worse than their phone.** ⭐ the repo if CamDroid made your setup better.

📜 [MIT License](LICENSE)

</div>
