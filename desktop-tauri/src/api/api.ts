import type {
  CameraCommand,
  ConnectRequest,
  DeviceInfo,
  FilterSettings,
  OutputMode,
  PresetInfo,
  RingLightSettings,
  SessionStatus,
  StudioState,
  SystemCheck,
} from "./types";

/** Everything the UI can ask of the backend. Implemented for Tauri and for a browser mock. */
export interface Api {
  getStudioState(): Promise<StudioState>;
  filterPresets(): Promise<PresetInfo[]>;
  systemCheck(): Promise<SystemCheck>;

  discoverDevices(timeoutSeconds?: number): Promise<DeviceInfo[]>;
  connect(request: ConnectRequest): Promise<SessionStatus>;
  /** `null` when there is no session. A dropped session is reported once, then released. */
  sessionStatus(): Promise<SessionStatus | null>;
  disconnect(): Promise<void>;
  /** Streams JPEG-encoded preview frames until the session ends. */
  startPreview(onFrame: (jpeg: ArrayBuffer) => void): Promise<void>;

  cameraCommand(command: CameraCommand): Promise<void>;
  setFilter(filter: FilterSettings): Promise<void>;
  setOutputMode(mode: OutputMode): Promise<void>;
  setMirror(enabled: boolean): Promise<void>;
  setRingLight(settings: RingLightSettings): Promise<void>;
  setRingLightVisible(visible: boolean): Promise<void>;

  quit(): Promise<void>;
}
