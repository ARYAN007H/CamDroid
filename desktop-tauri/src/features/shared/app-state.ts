import type {
  PresetInfo,
  SessionStatus,
  StudioState,
  SystemCheck,
  WhiteBalance,
} from "../../api/types";

export type ScreenName = "connect" | "studio";

/** Last values sent to the phone. The phone does not report them back. */
export interface CameraState {
  zoom: number;
  focusMode: "auto" | "manual";
  focusDistance: number;
  exposure: number;
  whiteBalance: WhiteBalance;
  flash: boolean;
}

export const DEFAULT_CAMERA: CameraState = {
  zoom: 1,
  focusMode: "auto",
  focusDistance: 0,
  exposure: 0,
  whiteBalance: "auto",
  flash: false,
};

export const ZOOM_RANGE = { min: 1, max: 10 } as const;

export interface AppState {
  screen: ScreenName;
  /** Set while a connection attempt is running; the label names the target. */
  connecting: { label: string } | null;
  connectError: string | null;
  studio: StudioState;
  presets: PresetInfo[];
  system: SystemCheck;
  session: SessionStatus | null;
  /** Increments on every successful connection, so views can restart per-session work. */
  sessionEpoch: number;
  /** Why the session ended, if it did; shows the "connection lost" overlay. */
  lostReason: string | null;
  camera: CameraState;
  trayOpen: boolean;
  panelOpen: boolean;
  helpOpen: boolean;
  fullscreen: boolean;
}
