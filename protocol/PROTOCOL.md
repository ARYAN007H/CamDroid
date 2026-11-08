# CamDroid Wire Protocol Specification v1.0

## Overview

CamDroid uses a custom binary protocol over TCP to stream video, audio, and control
commands between the Android phone (server) and the desktop client (receiver).

The protocol is designed for:
- **Minimal overhead**: 5-byte header per packet
- **Low latency**: No container format buffering
- **Bidirectional control**: Camera settings adjustable from desktop
- **Codec agnostic**: Same framing for H.264, H.265, and MJPEG

## Transport

- **WiFi**: TCP connection to phone's IP on port 4747 (configurable)
- **USB**: ADB TCP port forwarding (`adb forward tcp:4747 tcp:4747`), then connect to `127.0.0.1:4747`
- **Discovery**: mDNS service type `_camdroid._tcp.` with TXT records for capabilities

## Packet Format

All multi-byte integers are **little-endian**.

```
┌───────────────┬────────────────────┬──────────────────────────────┐
│ Type (1 byte) │ Length (4 bytes LE) │ Payload (Length bytes)        │
└───────────────┴────────────────────┴──────────────────────────────┘
```

### Fields

| Field  | Size    | Description                                    |
|--------|---------|------------------------------------------------|
| Type   | 1 byte  | Packet type identifier (see table below)       |
| Length | 4 bytes | Payload length in bytes (little-endian uint32)  |
| Payload| N bytes | Type-specific payload data                     |

Maximum payload size: 16 MB (16,777,216 bytes). Packets exceeding this are invalid.

## Packet Types

| Name          | Value  | Direction     | Description                              |
|---------------|--------|---------------|------------------------------------------|
| VIDEO_FRAME   | `0x01` | Phone → PC    | Encoded video frame (NAL unit or JPEG)   |
| AUDIO_FRAME   | `0x02` | Phone → PC    | Encoded audio frame (AAC with ADTS)      |
| CONTROL_CMD   | `0x03` | PC → Phone    | JSON-encoded control command             |
| METADATA      | `0x04` | Bidirectional | JSON-encoded metadata/configuration      |
| HEARTBEAT     | `0x05` | Bidirectional | Keepalive (empty payload)                |
| VIDEO_CONFIG  | `0x06` | Phone → PC    | Codec configuration data (SPS/PPS/VPS)   |
| AUDIO_CONFIG  | `0x07` | Phone → PC    | Audio configuration (AudioSpecificConfig)|

## Connection Lifecycle

### 1. Handshake

```
Client → Server:  METADATA { "version": "1.0", "client": "camdroid-desktop" }
Server → Client:  METADATA {
                    "version": "1.0",
                    "device": "Pixel 7",
                    "codecs": ["h264", "h265", "mjpeg"],
                    "resolutions": ["1080p", "1440p", "4k"],
                    "fps": [30, 60],
                    "audio": true,
                    "battery": 85
                  }
```

### 2. Stream Start

```
Client → Server:  CONTROL_CMD {
                    "cmd": "start",
                    "codec": "h264",
                    "resolution": "1080p",
                    "fps": 60,
                    "audio": true
                  }
Server → Client:  VIDEO_CONFIG [SPS + PPS bytes for H.264]
Server → Client:  AUDIO_CONFIG [AudioSpecificConfig bytes]
Server → Client:  VIDEO_FRAME [IDR frame (keyframe)]
Server → Client:  AUDIO_FRAME [AAC frame with ADTS header]
Server → Client:  VIDEO_FRAME [P-frame]
... (continuous streaming)
```

### 3. Heartbeat

Both sides send `HEARTBEAT` packets every 2 seconds. If no packet (of any type)
is received for 10 seconds, the connection is considered dead.

### 4. Graceful Disconnect

```
Client → Server:  CONTROL_CMD { "cmd": "stop" }
(Server stops streaming, closes connection)
```

## Video Frame Payload

### H.264 (AVC)
- Raw NAL units in Annex B format (prefixed with `00 00 00 01` start codes)
- Each VIDEO_FRAME packet contains exactly one NAL unit
- NAL types: SPS (7), PPS (8), IDR (5), non-IDR (1)
- SPS/PPS are sent in VIDEO_CONFIG at stream start and before each keyframe

### H.265 (HEVC)
- Raw NAL units in Annex B format
- Each VIDEO_FRAME packet contains exactly one NAL unit
- NAL types: VPS (32), SPS (33), PPS (34), IDR_W_RADL (19), IDR_N_LP (20)
- VPS/SPS/PPS are sent in VIDEO_CONFIG at stream start

### MJPEG
- Each VIDEO_FRAME packet contains a complete JPEG image
- No VIDEO_CONFIG needed (each frame is self-contained)
- SOI marker: `FF D8`, EOI marker: `FF D9`

## Audio Frame Payload

- AAC-LC encoded audio with ADTS header
- Sample rate: 44100 Hz
- Channels: 1 (mono)
- Bit rate: 128 kbps
- ADTS header: 7 bytes prepended to each raw AAC frame
- AUDIO_CONFIG contains the 2-byte AudioSpecificConfig for decoder initialization

## Control Commands

All control commands are JSON objects with a `cmd` field. Sent as CONTROL_CMD packets.

### Camera Control

```json
{ "cmd": "switch_camera" }
```
Switch between front and rear cameras. Causes a brief stream interruption
while the camera reinitializes. A new VIDEO_CONFIG will be sent after switching.

```json
{ "cmd": "set_zoom", "value": 2.5 }
```
Set zoom ratio. Value range depends on device (typically 1.0 to 10.0).
Values outside the supported range are clamped.

```json
{ "cmd": "set_focus", "mode": "auto" }
{ "cmd": "set_focus", "mode": "manual", "distance": 1.5 }
{ "cmd": "set_focus", "mode": "tap", "x": 0.5, "y": 0.3 }
```
Set focus mode. `distance` is in diopters (0.0 = infinity).
`tap` coordinates are normalized (0.0-1.0) relative to the preview.

```json
{ "cmd": "set_exposure", "compensation": 2 }
```
Set exposure compensation. Value is in EV steps (typically -4 to +4).

```json
{ "cmd": "set_white_balance", "mode": "auto" }
{ "cmd": "set_white_balance", "mode": "daylight" }
{ "cmd": "set_white_balance", "mode": "tungsten" }
{ "cmd": "set_white_balance", "mode": "fluorescent" }
{ "cmd": "set_white_balance", "mode": "cloudy" }
```

```json
{ "cmd": "set_flash", "enabled": true }
```
Toggle the camera torch/flashlight.

```json
{ "cmd": "set_mirror", "enabled": true }
```
Horizontally flip the video output.

### Stream Control

```json
{ "cmd": "set_resolution", "value": "4k" }
```
Change resolution. Valid values: `"1080p"`, `"1440p"`, `"4k"`.
Causes encoder restart. A new VIDEO_CONFIG will be sent.

```json
{ "cmd": "set_fps", "value": 30 }
```
Change frame rate. Valid values: 30, 60.

```json
{ "cmd": "set_codec", "value": "h265" }
```
Change codec. Valid values: `"h264"`, `"h265"`, `"mjpeg"`.
Causes encoder restart. A new VIDEO_CONFIG will be sent.

```json
{ "cmd": "set_bitrate", "value": 8000000 }
```
Set target bitrate in bits per second. Disables adaptive bitrate.
Set to 0 to re-enable adaptive bitrate.

```json
{ "cmd": "request_keyframe" }
```
Request an immediate I-frame from the encoder.

```json
{ "cmd": "start", "codec": "h264", "resolution": "1080p", "fps": 60, "audio": true }
{ "cmd": "stop" }
```

## Metadata Payload

JSON objects containing stream information. Sent periodically or on state changes.

### Server → Client Status Update
```json
{
  "type": "status",
  "fps": 59.8,
  "bitrate": 6200000,
  "resolution": "1920x1080",
  "codec": "h264",
  "battery": 72,
  "temperature": 38.5,
  "dropped_frames": 0
}
```

## Error Handling

- **Invalid packet type**: Log warning, skip packet
- **Payload too large**: Close connection (possible corruption)
- **Incomplete read**: Keep reading until full packet received (TCP framing)
- **Connection lost**: Client retries with exponential backoff
- **Unsupported command**: Server responds with METADATA error:
  ```json
  { "type": "error", "message": "Unsupported command: xyz" }
  ```

## mDNS Service Discovery

### Service Registration (Phone)
- Service type: `_camdroid._tcp.`
- Service name: `CamDroid-<device_name>`
- Port: 4747 (or configured port)
- TXT records:
  - `version=1.0`
  - `codecs=h264,h265,mjpeg`
  - `resolution=1080p`
  - `device=Pixel 7`

### Service Discovery (Desktop)
- Browse for `_camdroid._tcp.local.`
- Resolve to get IP address and port
- Read TXT records for device capabilities
