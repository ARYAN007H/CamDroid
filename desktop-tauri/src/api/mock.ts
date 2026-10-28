/**
 * A browser-only stand-in for the backend, used for development
 * (`npm run dev:mock`) and visual testing. It is excluded from desktop builds.
 */
import type { Api } from "./api";
import type { Platform } from "../core/platform";
import type {
  FilterPresetId,
  FilterSettings,
  PresetInfo,
  SessionStatus,
  StudioState,
} from "./types";

const settings = (preset: FilterPresetId, t = 0, s = 1, c = 1): FilterSettings => ({
  preset,
  strength: 1,
  temperature: t,
  saturation: s,
  contrast: c,
});

const PRESETS: PresetInfo[] = [
  { id: "natural", settings: settings("natural"), spatial: false },
  { id: "warm_film", settings: settings("warm_film", 0.28, 1.08, 1.06), spatial: false },
  { id: "teal_orange", settings: settings("teal_orange", 0.1, 1.2, 1.1), spatial: false },
  { id: "noir", settings: settings("noir", 0, 0, 1.28), spatial: false },
  { id: "vintage", settings: settings("vintage", 0.2, 0.82, 0.94), spatial: false },
  { id: "pixel", settings: settings("pixel"), spatial: true },
  { id: "vhs", settings: settings("vhs", 0.08, 1.15, 1.04), spatial: true },
  { id: "neon_edge", settings: settings("neon_edge", 0, 1.38, 1.28), spatial: true },
  { id: "cartoon", settings: settings("cartoon", 0.02, 1.3, 1.18), spatial: true },
];

const CSS_FILTERS: Partial<Record<FilterPresetId, string>> = {
  warm_film: "sepia(0.35) saturate(1.1)",
  teal_orange: "contrast(1.1) saturate(1.3) hue-rotate(-8deg)",
  noir: "grayscale(1) contrast(1.25)",
  vintage: "sepia(0.55) saturate(0.8)",
  vhs: "saturate(1.2) contrast(1.05)",
  neon_edge: "contrast(1.4) saturate(1.8) brightness(0.8)",
  cartoon: "saturate(1.6) contrast(1.2)",
};

const delay = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

export function createMockPlatform(): Platform {
  let studio: StudioState = {
    outputMode: "standalone",
    mirror: false,
    filter: PRESETS[0]!.settings,
    ringLight: { brightness: 0.85, warmth: 0.1, thickness: 0.09 },
    ringLightVisible: false,
    stream: { codec: "h264", resolution: "1080p", fps: 60, audio: true },
    wifi: { host: "192.168.1.67", port: 4747 },
  };
  let connectedAt: number | null = null;
  let previewTimer = 0;

  const status = (): SessionStatus => ({
    connected: connectedAt !== null,
    deviceName: "Pixel 8 Pro",
    codec: studio.stream.codec,
    width: 1920,
    height: 1080,
    fps: studio.stream.fps,
    audioEnabled: studio.stream.audio,
    telemetry: {
      framesDecoded: 1000,
      framesDropped: 3,
      decodeErrors: 0,
      renderFps: 58 + Math.random() * 2,
      batteryPercent: 72,
      virtualCamera:
        studio.outputMode === "virtual_camera"
          ? { state: "failed", detail: "Failed to open /dev/video10. Make sure v4l2loopback is loaded." }
          : { state: "inactive" },
      audioError: null,
      disconnectReason: null,
    },
  });

  const api: Api = {
    getStudioState: async () => structuredClone(studio),
    filterPresets: async () => structuredClone(PRESETS),
    systemCheck: async () => ({ adbAvailable: true, virtualCameraDevice: "/dev/video10", virtualCameraReady: true }),
    discoverDevices: async () => {
      await delay(900);
      return [
        { name: "CamDroid-Pixel 8 Pro", ip: "192.168.1.67", port: 4747, model: "Pixel 8 Pro", codecs: "h264,h265", resolution: "1080p" },
        { name: "CamDroid-OPPO A5x 5G", ip: "192.168.1.71", port: 4747, model: "OPPO A5x 5G", codecs: "h264", resolution: "1080p" },
      ];
    },
    connect: async () => {
      await delay(800);
      connectedAt = Date.now();
      return status();
    },
    sessionStatus: async () => (connectedAt === null ? null : status()),
    disconnect: async () => {
      connectedAt = null;
      window.clearInterval(previewTimer);
    },
    startPreview: async (onFrame) => {
      const canvas = document.createElement("canvas");
      canvas.width = 960;
      canvas.height = 540;
      const ctx = canvas.getContext("2d")!;
      window.clearInterval(previewTimer);
      previewTimer = window.setInterval(() => {
        const t = performance.now() / 1000;
        ctx.filter = "none";
        const sky = ctx.createLinearGradient(0, 0, 0, 540);
        sky.addColorStop(0, "#5b7da6");
        sky.addColorStop(0.6, "#e9c7a1");
        sky.addColorStop(1, "#3a2f2a");
        ctx.fillStyle = sky;
        ctx.fillRect(0, 0, 960, 540);
        ctx.fillStyle = "#d9b48b";
        ctx.beginPath();
        ctx.arc(480 + Math.sin(t) * 18, 215, 78, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = "#2c3e5a";
        ctx.beginPath();
        ctx.ellipse(480 + Math.sin(t) * 18, 470, 190, 150, 0, 0, Math.PI * 2);
        ctx.fill();
        const tint = CSS_FILTERS[studio.filter.preset];
        if (tint) {
          ctx.filter = tint;
          ctx.drawImage(canvas, 0, 0);
        }
        canvas.toBlob(async (blob) => {
          if (blob) onFrame(await blob.arrayBuffer());
        }, "image/jpeg", 0.8);
      }, 66);
    },
    cameraCommand: async () => {},
    setFilter: async (filter) => {
      studio = { ...studio, filter };
    },
    setOutputMode: async (outputMode) => {
      studio = { ...studio, outputMode };
    },
    setMirror: async (mirror) => {
      studio = { ...studio, mirror };
    },
    setRingLight: async (ringLight) => {
      studio = { ...studio, ringLight };
    },
    setRingLightVisible: async (ringLightVisible) => {
      studio = { ...studio, ringLightVisible };
    },
    quit: async () => {},
  };

  return {
    api,
    mock: true,
    window: {
      show: async () => {},
      setFullscreen: async (on) => {
        if (on) await document.documentElement.requestFullscreen?.().catch(() => {});
        else if (document.fullscreenElement) await document.exitFullscreen().catch(() => {});
      },
      isFullscreen: async () => document.fullscreenElement !== null,
      startResize: async () => {},
    },
  };
}
