import type { Api } from "../api/api";

export type ResizeDirection =
  | "North"
  | "South"
  | "East"
  | "West"
  | "NorthEast"
  | "NorthWest"
  | "SouthEast"
  | "SouthWest";

/** What the UI needs from the host window (it has no title bar of its own). */
export interface WindowControls {
  /** Reveals the window; it starts hidden so the first paint is never a white flash. */
  show(): Promise<void>;
  setFullscreen(on: boolean): Promise<void>;
  isFullscreen(): Promise<boolean>;
  startResize(direction: ResizeDirection): Promise<void>;
}

/** Everything that differs between the desktop app and a plain browser. */
export interface Platform {
  api: Api;
  window: WindowControls;
  /** True when running without a backend (development and visual testing). */
  mock: boolean;
}

/** Chooses the real Tauri platform, or the in-browser mock when built with VITE_MOCK. */
export async function loadPlatform(): Promise<Platform> {
  if (import.meta.env.VITE_MOCK) {
    const { createMockPlatform } = await import("../api/mock");
    return createMockPlatform();
  }
  const [{ tauriApi }, { getCurrentWindow }] = await Promise.all([
    import("../api/tauri"),
    import("@tauri-apps/api/window"),
  ]);
  const win = getCurrentWindow();
  return {
    api: tauriApi,
    mock: false,
    window: {
      show: () => win.show(),
      setFullscreen: (on) => win.setFullscreen(on),
      isFullscreen: () => win.isFullscreen(),
      startResize: (direction) => win.startResizeDragging(direction),
    },
  };
}
