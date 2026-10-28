import { h } from "../core/dom";
import { Keymap } from "../core/keyboard";
import type { Platform, ResizeDirection } from "../core/platform";
import { createToaster } from "../ui/toast";
import { createConnectScreen } from "./connect/connect-screen";
import { DEFAULT_CAMERA, type AppState, type ScreenName } from "./shared/app-state";
import { Controller } from "./shared/controller";
import { createHelpOverlay } from "./shared/help-overlay";
import type { Screen } from "./shared/screen";
import { createStudioScreen } from "./studio/studio-screen";

const RESIZE_DIRECTIONS: readonly ResizeDirection[] = [
  "North", "South", "East", "West", "NorthEast", "NorthWest", "SouthEast", "SouthWest",
];

/** Boots the application into `root`. */
export async function startApp(root: HTMLElement, platform: Platform): Promise<void> {
  const { api } = platform;
  const [studio, presets, system] = await Promise.all([
    api.getStudioState(),
    api.filterPresets(),
    api.systemCheck(),
  ]);

  const initial: AppState = {
    screen: "connect",
    connecting: null,
    connectError: null,
    studio,
    presets,
    system,
    session: null,
    sessionEpoch: 0,
    lostReason: null,
    camera: DEFAULT_CAMERA,
    trayOpen: false,
    panelOpen: false,
    helpOpen: false,
    fullscreen: false,
  };

  const toast = createToaster(root);
  const controller = new Controller(initial, platform, toast);
  const keymap = registerShortcuts(controller);
  window.addEventListener("keydown", (event) => void keymap.handle(event));

  // Screens ------------------------------------------------------------------------
  const screenHost = h("div", { attrs: { style: "position:absolute;inset:0" } });
  root.append(screenHost);
  let current: { name: ScreenName; screen: Screen } | null = null;

  const mount = (name: ScreenName): void => {
    current?.screen.destroy();
    screenHost.replaceChildren();
    const screen = name === "studio" ? createStudioScreen(controller) : createConnectScreen(controller);
    screenHost.append(screen.el);
    screen.focus?.();
    current = { name, screen };
    document.querySelector(".toast")?.classList.toggle("on-connect", name === "connect");
  };
  controller.store.subscribe((state) => {
    if (state.screen !== current?.name) mount(state.screen);
  });
  mount(initial.screen);

  const help = createHelpOverlay(controller, keymap);
  root.append(help.el, ...resizeGrips(platform));

  document.addEventListener("contextmenu", (event) => event.preventDefault());
  await document.fonts.ready.catch(() => undefined);
  await platform.window.show();
}

function resizeGrips(platform: Platform): HTMLElement[] {
  return RESIZE_DIRECTIONS.map((direction) =>
    h("div", {
      class: "resize-grip",
      attrs: { "data-dir": direction },
      on: {
        mousedown: (event) => {
          if (event.button !== 0) return;
          event.preventDefault();
          void platform.window.startResize(direction);
        },
      },
    }),
  );
}

function registerShortcuts(controller: Controller): Keymap {
  const keymap = new Keymap();
  const inStudio = (): boolean => controller.store.get().screen === "studio";
  const live = (): boolean => inStudio() && controller.store.get().lostReason === null;

  keymap.add(
    { keys: ["ctrl+q", "ctrl+w"], group: "General", description: "Quit CamDroid", global: true, handler: () => void controller.quit() },
    { keys: ["escape"], group: "General", description: "Close panel, exit full screen", global: true, handler: () => void controller.dismiss() },
    { keys: ["?", "f1"], group: "General", description: "Show shortcuts", handler: () => controller.toggleHelp() },
    { keys: ["f", "f11"], group: "General", description: "Full screen", handler: () => void controller.toggleFullscreen() },
    { keys: ["ctrl+d"], group: "General", description: "Disconnect", when: inStudio, global: true, handler: () => void controller.disconnect() },

    { keys: ["e"], group: "Studio", description: "Looks", when: live, handler: () => controller.toggleTray() },
    { keys: ["i"], group: "Studio", description: "Adjust panel", when: live, handler: () => controller.togglePanel() },
    { keys: ["m"], group: "Studio", description: "Mirror", when: live, handler: () => void controller.toggleMirror() },
    { keys: ["r"], group: "Studio", description: "Ring light", when: live, handler: () => void controller.toggleRingLight() },
    { keys: ["o"], group: "Studio", description: "Virtual camera", when: live, handler: () => void controller.toggleVirtualCamera() },

    { keys: ["+", "="], group: "Phone camera", description: "Zoom in", when: live, handler: () => controller.nudgeZoom(1) },
    { keys: ["-"], group: "Phone camera", description: "Zoom out", when: live, handler: () => controller.nudgeZoom(-1) },
    { keys: ["0"], group: "Phone camera", description: "Reset zoom", when: live, handler: () => controller.resetZoom() },
    { keys: ["c"], group: "Phone camera", description: "Switch front / back camera", when: live, handler: () => controller.switchCamera() },

    { keys: ["]"], group: "Looks", description: "Next look", when: live, handler: () => controller.cyclePreset(1) },
    { keys: ["["], group: "Looks", description: "Previous look", when: live, handler: () => controller.cyclePreset(-1) },
  );
  for (let index = 0; index < 9; index++) {
    keymap.add({
      keys: [String(index + 1)],
      group: "Looks",
      description: index === 0 ? "Pick a look by number (1 to 9)" : "",
      hidden: true,
      when: live,
      handler: () => controller.selectPresetAt(index),
    });
  }
  return keymap;
}
