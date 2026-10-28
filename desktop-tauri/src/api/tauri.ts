import { Channel, invoke } from "@tauri-apps/api/core";
import type { Api } from "./api";

/** The real backend: thin, typed wrappers over Tauri commands. */
export const tauriApi: Api = {
  getStudioState: () => invoke("get_studio_state"),
  filterPresets: () => invoke("filter_presets"),
  systemCheck: () => invoke("system_check"),

  discoverDevices: (timeoutSeconds) => invoke("discover_devices", { timeoutSeconds }),
  connect: (request) => invoke("connect", { request }),
  sessionStatus: () => invoke("session_status"),
  disconnect: () => invoke("disconnect"),
  async startPreview(onFrame) {
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = onFrame;
    await invoke("start_preview", { onFrame: channel });
  },

  cameraCommand: (command) => invoke("camera_command", { command }),
  setFilter: (filter) => invoke("set_filter", { filter }),
  setOutputMode: (mode) => invoke("set_output_mode", { mode }),
  setMirror: (enabled) => invoke("set_mirror", { enabled }),
  setRingLight: (settings) => invoke("set_ring_light", { settings }),
  setRingLightVisible: (visible) => invoke("set_ring_light_visible", { visible }),

  quit: () => invoke("quit_app"),
};
