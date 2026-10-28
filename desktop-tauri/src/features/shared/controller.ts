import type { Api } from "../../api/api";
import type {
  CameraCommand,
  ConnectRequest,
  FilterPresetId,
  FilterSettings,
  RingLightSettings,
  StreamPreferences,
  WhiteBalance,
} from "../../api/types";
import { clamp, errorMessage, formatCodec, rafThrottle } from "../../core/format";
import type { Platform } from "../../core/platform";
import { createStore, type Store } from "../../core/store";
import type { Toaster } from "../../ui/toast";
import { DEFAULT_CAMERA, ZOOM_RANGE, type AppState, type CameraState } from "./app-state";

const STATUS_POLL_MS = 1000;
const ZOOM_STEP = 0.5;

/**
 * Every user-facing action lives here. Screens render state and call these
 * methods; this is the only place that talks to the backend, so error
 * handling and optimistic updates are consistent.
 */
export class Controller {
  readonly store: Store<AppState>;
  private readonly api: Api;
  private lastRequest: { request: ConnectRequest; label: string } | null = null;
  private attempt = 0;
  private pendingConnect: Promise<unknown> = Promise.resolve();
  private pollTimer = 0;

  private readonly pushFilter = rafThrottle(() => {
    void this.guard(this.api.setFilter(this.store.get().studio.filter));
  });
  private readonly pushRingLight = rafThrottle(() => {
    void this.guard(this.api.setRingLight(this.store.get().studio.ringLight));
  });
  private readonly pushZoom = rafThrottle(() => {
    void this.sendCamera({ type: "zoom", value: this.store.get().camera.zoom });
  });
  private readonly pushExposure = rafThrottle(() => {
    void this.sendCamera({ type: "exposure", compensation: this.store.get().camera.exposure });
  });
  private readonly pushFocus = rafThrottle(() => {
    void this.sendCamera({ type: "focus_manual", distance: this.store.get().camera.focusDistance });
  });

  constructor(
    initial: AppState,
    readonly platform: Platform,
    readonly toast: Toaster,
  ) {
    this.store = createStore(initial);
    this.api = platform.api;
  }

  /** Runs a backend call; failures become an error toast instead of an unhandled rejection. */
  private async guard<T>(call: Promise<T>): Promise<T | undefined> {
    try {
      return await call;
    } catch (error) {
      this.toast.show(errorMessage(error), "error");
      return undefined;
    }
  }

  // Connection -------------------------------------------------------------

  async connect(request: ConnectRequest, label: string): Promise<void> {
    if (this.store.get().connecting) return;
    const attempt = ++this.attempt;
    this.lastRequest = { request, label };
    this.store.set({ connecting: { label }, connectError: null });

    // A cancelled attempt may still be running in the backend; wait it out.
    await this.pendingConnect.catch(() => undefined);
    if (attempt !== this.attempt) return;

    const running = this.api.connect(request);
    this.pendingConnect = running;
    try {
      const session = await running;
      if (attempt !== this.attempt) {
        await this.api.disconnect().catch(() => undefined);
        return;
      }
      const studio = await this.api.getStudioState();
      this.store.set({
        studio,
        session,
        sessionEpoch: this.store.get().sessionEpoch + 1,
        screen: "studio",
        connecting: null,
        lostReason: null,
        camera: DEFAULT_CAMERA,
        trayOpen: false,
        panelOpen: false,
      });
    } catch (error) {
      if (attempt === this.attempt) {
        this.store.set({ connecting: null, connectError: errorMessage(error) });
      }
    }
  }

  cancelConnect(): void {
    this.attempt++;
    this.store.set({ connecting: null });
  }

  /** Reconnects to the same phone with the same settings. */
  async reconnect(): Promise<void> {
    const last = this.lastRequest;
    if (!last) {
      this.leaveStudio();
      return;
    }
    this.store.set({ lostReason: null });
    await this.connect(last.request, last.label);
    if (this.store.get().connectError) this.leaveStudio();
  }

  async disconnect(): Promise<void> {
    this.stopWatching();
    await this.guard(this.api.disconnect());
    this.leaveStudio();
  }

  /** Back to the connection screen, keeping any error to explain why. */
  private leaveStudio(): void {
    this.stopWatching();
    this.store.set({
      screen: "connect",
      session: null,
      lostReason: null,
      trayOpen: false,
      panelOpen: false,
    });
  }

  /** Polls session health while the studio is open. */
  startWatching(): void {
    this.stopWatching();
    this.pollTimer = window.setInterval(() => void this.poll(), STATUS_POLL_MS);
  }

  stopWatching(): void {
    window.clearInterval(this.pollTimer);
    this.pollTimer = 0;
  }

  private async poll(): Promise<void> {
    try {
      const status = await this.api.sessionStatus();
      if (status === null) {
        this.store.set({ lostReason: this.store.get().lostReason ?? "The session has ended." });
        this.stopWatching();
      } else if (!status.connected) {
        this.store.set({
          session: status,
          lostReason: status.telemetry.disconnectReason ?? "The connection to the phone was lost.",
        });
        this.stopWatching();
      } else {
        this.store.set({ session: status });
      }
    } catch {
      // A failed poll is not a lost session; try again on the next tick.
    }
  }

  updateStreamPreferences(patch: Partial<StreamPreferences>): void {
    const { studio } = this.store.get();
    this.store.set({ studio: { ...studio, stream: { ...studio.stream, ...patch } } });
  }

  // Look --------------------------------------------------------------------

  selectPreset(id: FilterPresetId, announce = true): void {
    const preset = this.store.get().presets.find((candidate) => candidate.id === id);
    if (!preset) return;
    this.setFilter(preset.settings);
    if (announce) this.toast.show(`Look: ${presetName(id)}`);
  }

  selectPresetAt(index: number): void {
    const preset = this.store.get().presets[index];
    if (preset) this.selectPreset(preset.id);
  }

  cyclePreset(delta: number): void {
    const { presets, studio } = this.store.get();
    if (presets.length === 0) return;
    const current = presets.findIndex((preset) => preset.id === studio.filter.preset);
    const next = (Math.max(current, 0) + delta + presets.length) % presets.length;
    this.selectPresetAt(next);
  }

  updateFilter(patch: Partial<FilterSettings>): void {
    this.setFilter({ ...this.store.get().studio.filter, ...patch });
  }

  /** Returns the current preset's sliders to their starting values. */
  resetFilter(): void {
    this.selectPreset(this.store.get().studio.filter.preset, false);
    this.toast.show("Look reset");
  }

  private setFilter(filter: FilterSettings): void {
    const { studio } = this.store.get();
    this.store.set({ studio: { ...studio, filter } });
    this.pushFilter();
  }

  // Output ------------------------------------------------------------------

  async toggleMirror(): Promise<void> {
    const { studio } = this.store.get();
    const mirror = !studio.mirror;
    this.store.set({ studio: { ...studio, mirror } });
    await this.guard(this.api.setMirror(mirror));
    this.toast.show(mirror ? "Mirror on" : "Mirror off");
  }

  async toggleVirtualCamera(): Promise<void> {
    const { studio, system } = this.store.get();
    const outputMode = studio.outputMode === "virtual_camera" ? "standalone" : "virtual_camera";
    this.store.set({ studio: { ...studio, outputMode } });
    await this.guard(this.api.setOutputMode(outputMode));
    if (outputMode === "virtual_camera" && !system.virtualCameraReady) {
      this.toast.show(`${system.virtualCameraDevice} not found. Run desktop/setup.sh`, "error");
    } else {
      this.toast.show(outputMode === "virtual_camera" ? "Virtual camera on" : "Virtual camera off");
    }
  }

  async toggleRingLight(): Promise<void> {
    const { studio } = this.store.get();
    const ringLightVisible = !studio.ringLightVisible;
    const done = await this.guard(this.api.setRingLightVisible(ringLightVisible));
    if (done === undefined && ringLightVisible) return; // failed to show; leave the state alone
    this.store.set({ studio: { ...this.store.get().studio, ringLightVisible } });
    this.toast.show(ringLightVisible ? "Ring light on" : "Ring light off");
  }

  updateRingLight(patch: Partial<RingLightSettings>): void {
    const { studio } = this.store.get();
    this.store.set({ studio: { ...studio, ringLight: { ...studio.ringLight, ...patch } } });
    this.pushRingLight();
  }

  // Phone camera --------------------------------------------------------------

  private async sendCamera(command: CameraCommand): Promise<void> {
    await this.guard(this.api.cameraCommand(command));
  }

  private updateCamera(patch: Partial<CameraState>): void {
    this.store.set({ camera: { ...this.store.get().camera, ...patch } });
  }

  setZoom(value: number): void {
    this.updateCamera({ zoom: clamp(Math.round(value * 10) / 10, ZOOM_RANGE.min, ZOOM_RANGE.max) });
    this.pushZoom();
  }

  nudgeZoom(direction: 1 | -1): void {
    this.setZoom(this.store.get().camera.zoom + direction * ZOOM_STEP);
  }

  resetZoom(): void {
    this.setZoom(1);
    this.toast.show("Zoom 1.0×");
  }

  setExposure(value: number): void {
    this.updateCamera({ exposure: clamp(Math.round(value), -4, 4) });
    this.pushExposure();
  }

  setFocusMode(mode: "auto" | "manual"): void {
    this.updateCamera({ focusMode: mode });
    if (mode === "auto") void this.sendCamera({ type: "focus_auto" });
    else this.pushFocus();
  }

  setFocusDistance(distance: number): void {
    this.updateCamera({ focusMode: "manual", focusDistance: clamp(distance, 0, 10) });
    this.pushFocus();
  }

  setWhiteBalance(mode: WhiteBalance): void {
    this.updateCamera({ whiteBalance: mode });
    void this.sendCamera({ type: "white_balance", mode });
  }

  toggleFlash(): void {
    const flash = !this.store.get().camera.flash;
    this.updateCamera({ flash });
    void this.sendCamera({ type: "flash", enabled: flash });
  }

  switchCamera(): void {
    void this.sendCamera({ type: "switch_camera" });
    this.toast.show("Switching camera…");
  }

  // Chrome -------------------------------------------------------------------

  toggleTray(): void {
    this.store.set({ trayOpen: !this.store.get().trayOpen });
  }

  togglePanel(): void {
    this.store.set({ panelOpen: !this.store.get().panelOpen });
  }

  toggleHelp(): void {
    this.store.set({ helpOpen: !this.store.get().helpOpen });
  }

  async toggleFullscreen(): Promise<void> {
    const fullscreen = !this.store.get().fullscreen;
    await this.guard(this.platform.window.setFullscreen(fullscreen));
    this.store.set({ fullscreen });
    document.documentElement.classList.toggle("app-fullscreen", fullscreen);
  }

  /**
   * Escape peels back one layer at a time: help, effects tray, adjust panel,
   * fullscreen, then a pending connection. Returns false if nothing was open.
   */
  dismiss(): boolean {
    const state = this.store.get();
    if (state.helpOpen) this.store.set({ helpOpen: false });
    else if (state.trayOpen) this.store.set({ trayOpen: false });
    else if (state.panelOpen) this.store.set({ panelOpen: false });
    else if (state.fullscreen) void this.toggleFullscreen();
    else if (state.connecting) this.cancelConnect();
    else return false;
    return true;
  }

  async quit(): Promise<void> {
    await this.guard(this.api.quit());
  }
}

const PRESET_NAMES: Record<FilterPresetId, string> = {
  natural: "Natural",
  warm_film: "Warm Film",
  teal_orange: "Teal & Orange",
  noir: "Noir",
  vintage: "Vintage",
  pixel: "Pixel",
  vhs: "VHS",
  neon_edge: "Neon Edge",
  cartoon: "Cartoon",
};

export function presetName(id: FilterPresetId): string {
  return PRESET_NAMES[id];
}

/** "1080p · 60 fps · H.264" for the stream the phone agreed to. */
export function describeStream(session: { height: number; fps: number; codec: string }): string {
  return `${session.height}p · ${session.fps} fps · ${formatCodec(session.codec)}`;
}
