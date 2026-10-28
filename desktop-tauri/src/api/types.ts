/** Wire types shared with the Rust backend (camelCase JSON). */

export type FilterPresetId =
  | "natural"
  | "warm_film"
  | "teal_orange"
  | "noir"
  | "vintage"
  | "pixel"
  | "vhs"
  | "neon_edge"
  | "cartoon";

export interface FilterSettings {
  preset: FilterPresetId;
  /** 0..1, how strongly the look is applied. */
  strength: number;
  /** -1 (cool) .. 1 (warm). */
  temperature: number;
  /** 0..2.5, 1 is unchanged. */
  saturation: number;
  /** 0.5..2, 1 is unchanged. */
  contrast: number;
}

export interface PresetInfo {
  id: FilterPresetId;
  settings: FilterSettings;
  /** Reshapes the picture (more CPU) rather than only grading colour. */
  spatial: boolean;
}

export type OutputMode = "standalone" | "virtual_camera";

export interface RingLightSettings {
  brightness: number;
  warmth: number;
  thickness: number;
}

export type Codec = "h264" | "h265" | "mjpeg";
export type Resolution = "1080p" | "1440p" | "4k";
export type FrameRate = 30 | 60;

export interface StreamPreferences {
  codec: Codec;
  resolution: Resolution;
  fps: FrameRate;
  audio: boolean;
}

export interface WifiTarget {
  host: string;
  port: number;
}

export interface StudioState {
  outputMode: OutputMode;
  mirror: boolean;
  filter: FilterSettings;
  ringLight: RingLightSettings;
  ringLightVisible: boolean;
  stream: StreamPreferences;
  wifi: WifiTarget;
}

export type VirtualCameraStatus =
  | { state: "inactive" }
  | { state: "active" }
  | { state: "failed"; detail: string };

export interface Telemetry {
  framesDecoded: number;
  framesDropped: number;
  decodeErrors: number;
  renderFps: number;
  batteryPercent: number | null;
  virtualCamera: VirtualCameraStatus;
  audioError: string | null;
  disconnectReason: string | null;
}

export interface SessionStatus {
  connected: boolean;
  deviceName: string;
  codec: string;
  width: number;
  height: number;
  fps: number;
  audioEnabled: boolean;
  telemetry: Telemetry;
}

export interface DeviceInfo {
  name: string;
  ip: string;
  port: number;
  model: string;
  codecs: string;
  resolution: string;
}

export type ConnectTarget =
  | { mode: "discover" }
  | { mode: "usb" }
  | { mode: "wifi"; host: string; port: number };

export interface ConnectRequest {
  target: ConnectTarget;
  stream: StreamPreferences;
}

export type WhiteBalance = "auto" | "daylight" | "tungsten" | "fluorescent" | "cloudy";

export type CameraCommand =
  | { type: "zoom"; value: number }
  | { type: "focus_auto" }
  | { type: "focus_manual"; distance: number }
  | { type: "focus_tap"; x: number; y: number }
  | { type: "exposure"; compensation: number }
  | { type: "white_balance"; mode: WhiteBalance }
  | { type: "flash"; enabled: boolean }
  | { type: "switch_camera" }
  | { type: "request_keyframe" };

export interface SystemCheck {
  adbAvailable: boolean;
  virtualCameraDevice: string;
  virtualCameraReady: boolean;
}
