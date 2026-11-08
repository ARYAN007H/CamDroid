import { h, toggleClass } from "../../core/dom";
import { formatCount } from "../../core/format";
import { icon } from "../../ui/icons";
import type { AppState } from "../shared/app-state";
import { describeStream, type Controller } from "../shared/controller";

/** Status along the top edge: who is connected, at what quality, how healthy. */
export function createHud(controller: Controller): { el: HTMLElement; destroy(): void } {
  const dot = h("span", { class: "dot live pulse" });
  const name = h("span");
  const meta = h("span", { class: "meta" });
  const fps = h("span", { class: "tabular" });
  const fpsPill = h("div", { class: "pill", attrs: { title: "Frames rendered per second" } }, fps);
  const batteryText = h("span", { class: "tabular" });
  const batteryPill = h("div", { class: "pill", attrs: { title: "Phone battery" } }, icon("battery", 16), batteryText);
  const audioPill = h("div", { class: "pill warn" }, icon("alert", 14), "No audio");
  const droppedPill = h("div", { class: "pill warn" });

  const disconnect = h(
    "button",
    {
      class: "icon-btn",
      attrs: { type: "button", "aria-label": "Disconnect", "data-tip": "Disconnect  Ctrl+D", "data-tip-below": true },
      on: { click: () => void controller.disconnect() },
    },
    icon("power", 20),
  );

  const el = h(
    "header",
    { class: "hud" },
    h("div", { class: "hud-left" }, h("div", { class: "pill" }, dot, name, meta)),
    h("div", { class: "hud-right" }, droppedPill, audioPill, batteryPill, fpsPill, disconnect),
  );

  function sync(state: Readonly<AppState>): void {
    const { session } = state;
    if (!session) return;
    const { telemetry } = session;
    name.textContent = session.deviceName;
    meta.textContent = describeStream(session);
    dot.className = `dot ${session.connected ? "live pulse" : "warn"}`;

    fps.textContent = `${Math.round(telemetry.renderFps)} fps`;

    batteryPill.hidden = telemetry.batteryPercent === null;
    batteryText.textContent = `${telemetry.batteryPercent ?? 0}%`;

    audioPill.hidden = telemetry.audioError === null;
    audioPill.title = telemetry.audioError ?? "";

    // Only flag drops once they are frequent enough to affect the picture.
    const dropRate = telemetry.framesDecoded > 120 ? telemetry.framesDropped / telemetry.framesDecoded : 0;
    toggleClass(droppedPill, "hidden", dropRate < 0.1);
    droppedPill.hidden = dropRate < 0.1;
    droppedPill.textContent = `${formatCount(telemetry.framesDropped)} dropped`;
  }

  const unsubscribe = controller.store.subscribe(sync);
  sync(controller.store.get());
  return { el, destroy: unsubscribe };
}
