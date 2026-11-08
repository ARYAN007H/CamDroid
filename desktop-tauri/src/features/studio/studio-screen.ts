import type { Controller } from "../shared/controller";
import type { Screen } from "../shared/screen";
import { createAdjustPanel } from "./adjust-panel";
import { createDock } from "./dock";
import { createEffectsTray } from "./effects-tray";
import { createHud } from "./hud";
import { createLostOverlay } from "./lost-overlay";
import { createViewfinder } from "./viewfinder";
import { h } from "../../core/dom";

const IDLE_AFTER_MS = 2800;

/** The live view: picture first, controls fade away when you stop using them. */
export function createStudioScreen(controller: Controller): Screen {
  const { store } = controller;
  const viewfinder = createViewfinder(controller);
  const hud = createHud(controller);
  const tray = createEffectsTray(controller);
  const dock = createDock(controller);
  const lost = createLostOverlay(controller);
  const panel = createAdjustPanel(controller);

  const stage = h("div", { class: "stage" }, viewfinder.el, hud.el, tray.el, dock.el, lost.el);
  const el = h("section", { class: "studio" }, stage, panel.el);

  // Idle: hide overlays and the cursor, like a video player. -----------------
  let idleTimer = 0;
  const canIdle = (): boolean => {
    const state = store.get();
    return !state.trayOpen && !state.panelOpen && !state.helpOpen && state.lostReason === null;
  };
  const wake = (): void => {
    el.classList.remove("idle");
    window.clearTimeout(idleTimer);
    idleTimer = window.setTimeout(() => {
      if (canIdle()) el.classList.add("idle");
    }, IDLE_AFTER_MS);
  };
  const activity: Array<keyof WindowEventMap> = ["pointermove", "pointerdown", "keydown", "wheel"];
  for (const type of activity) window.addEventListener(type, wake, { passive: true });

  // Layout classes follow state. ---------------------------------------------
  let epoch = store.get().sessionEpoch;
  const unsubscribe = store.subscribe((state) => {
    el.classList.toggle("panel-open", state.panelOpen);
    el.classList.toggle("tray-open", state.trayOpen);
    if (!canIdle()) wake();
    if (state.sessionEpoch !== epoch) {
      // A reconnect produced a new session with a new preview stream.
      epoch = state.sessionEpoch;
      void viewfinder.start();
      controller.startWatching();
    }
  });
  const initial = store.get();
  el.classList.toggle("panel-open", initial.panelOpen);
  el.classList.toggle("tray-open", initial.trayOpen);

  void viewfinder.start();
  controller.startWatching();
  wake();

  return {
    el,
    destroy() {
      window.clearTimeout(idleTimer);
      for (const type of activity) window.removeEventListener(type, wake);
      unsubscribe();
      controller.stopWatching();
      for (const part of [viewfinder, hud, tray, dock, lost, panel]) part.destroy();
    },
  };
}
