import { h } from "../../core/dom";
import type { AppState } from "../shared/app-state";
import { presetName, type Controller } from "../shared/controller";
import { PRESET_SWATCHES } from "./presets";

/** A row of looks above the dock. Number keys 1-9 pick them directly. */
export function createEffectsTray(controller: Controller): { el: HTMLElement; destroy(): void } {
  const tiles = controller.store.get().presets.map((preset, index) => {
    const swatch = h("span", { class: "tile-swatch" }, h("span", { class: "tile-num" }, String(index + 1)));
    swatch.style.background = PRESET_SWATCHES[preset.id];
    const tile = h(
      "button",
      {
        class: "tile",
        attrs: { type: "button", role: "radio", "aria-checked": false, "aria-label": presetName(preset.id) },
        on: { click: () => controller.selectPreset(preset.id) },
      },
      swatch,
      h("span", { class: "tile-name" }, presetName(preset.id)),
    );
    return { id: preset.id, tile };
  });

  const el = h(
    "div",
    { class: "tray", attrs: { role: "radiogroup", "aria-label": "Looks" } },
    ...tiles.map((entry) => entry.tile),
  );

  function sync(state: Readonly<AppState>): void {
    for (const { id, tile } of tiles) {
      tile.setAttribute("aria-checked", String(id === state.studio.filter.preset));
    }
    // Hidden trays must not be reachable by keyboard.
    for (const { tile } of tiles) tile.tabIndex = state.trayOpen ? 0 : -1;
  }

  const unsubscribe = controller.store.subscribe(sync);
  sync(controller.store.get());
  return { el, destroy: unsubscribe };
}
