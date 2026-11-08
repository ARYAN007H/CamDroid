import { h } from "../../core/dom";
import { icon } from "../../ui/icons";
import type { AppState } from "../shared/app-state";
import type { Controller } from "../shared/controller";

/** Shown over the last picture when the phone disconnects or goes silent. */
export function createLostOverlay(controller: Controller): { el: HTMLElement; destroy(): void } {
  const reason = h("p");
  const reconnect = h(
    "button",
    { class: "btn btn-primary", attrs: { type: "button" }, on: { click: () => void controller.reconnect() } },
    icon("refresh", 16),
    "Reconnect",
  );
  const back = h(
    "button",
    { class: "btn btn-secondary", attrs: { type: "button" }, on: { click: () => void controller.disconnect() } },
    "Back",
  );
  const el = h(
    "div",
    { class: "lost", attrs: { role: "alertdialog", "aria-label": "Connection lost" } },
    h(
      "div",
      { class: "lost-card" },
      h("div", { class: "glyph" }, icon("unplugged", 24)),
      h("h2", {}, "Connection lost"),
      reason,
      h("div", { class: "lost-actions" }, back, reconnect),
    ),
  );

  let wasVisible = false;
  function sync(state: Readonly<AppState>): void {
    const visible = state.lostReason !== null;
    el.classList.toggle("visible", visible);
    reason.textContent = state.lostReason ?? "";
    if (visible && !wasVisible) reconnect.focus();
    wasVisible = visible;
  }

  const unsubscribe = controller.store.subscribe(sync);
  sync(controller.store.get());
  return { el, destroy: unsubscribe };
}
