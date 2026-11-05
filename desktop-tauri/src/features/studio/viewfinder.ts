import { h } from "../../core/dom";
import { computeFeedTransform } from "./feed-geometry";
import type { Controller } from "../shared/controller";

const NO_PICTURE_HINT_MS = 6000;

/**
 * The live picture. Preview frames arrive as JPEGs; only the newest is ever
 * decoded, so a slow frame costs a skipped frame rather than growing delay.
 */
export function createViewfinder(controller: Controller): { el: HTMLElement; start(): Promise<void>; destroy(): void } {
  const canvas = h("canvas", { class: "frame" });
  const context = canvas.getContext("2d", { alpha: false });
  const hint = h("p", { class: "sub" });
  const waiting = h("div", { class: "waiting" }, h("div", { class: "spinner" }), h("p", {}, "Waiting for video…"), hint);
  // The whole area drags the window; the canvas ignores the mouse so drags reach it.
  const el = h("div", { class: "viewfinder", attrs: { "data-tauri-drag-region": true } }, canvas, waiting);

  let latest: ArrayBuffer | null = null;
  let decoding = false;
  let gotFrame = false;
  let hintTimer = 0;
  let destroyed = false;

  async function drain(): Promise<void> {
    decoding = true;
    while (latest && !destroyed) {
      const buffer = latest;
      latest = null;
      try {
        const bitmap = await createImageBitmap(new Blob([buffer], { type: "image/jpeg" }));
        // The stage rotation/crop is baked into the bitmap so the existing
        // `object-fit: contain` letterboxes every combination for free.
        const studio = controller.store.get().studio;
        const t = computeFeedTransform(
          bitmap.width,
          bitmap.height,
          studio.aspect ?? "source",
          (studio.rotation ?? 0) as Parameters<typeof computeFeedTransform>[3],
        );
        if (canvas.width !== t.width || canvas.height !== t.height) {
          canvas.width = t.width;
          canvas.height = t.height;
        }
        if (context) {
          context.setTransform(1, 0, 0, 1, 0, 0);
          context.clearRect(0, 0, t.width, t.height);
          context.translate(t.width / 2, t.height / 2);
          context.rotate((t.rotation * Math.PI) / 180);
          context.drawImage(bitmap, t.sx, t.sy, t.sw, t.sh, -t.sw / 2, -t.sh / 2, t.sw, t.sh);
          context.setTransform(1, 0, 0, 1, 0, 0);
        }
        bitmap.close();
        if (!gotFrame) {
          gotFrame = true;
          canvas.classList.add("live");
          waiting.classList.add("hidden");
          window.clearTimeout(hintTimer);
        }
      } catch {
        // A corrupt frame is skipped; the next one replaces it.
      }
    }
    decoding = false;
  }

  function onFrame(jpeg: ArrayBuffer): void {
    latest = jpeg;
    if (!decoding) void drain();
  }

  return {
    el,
    async start() {
      gotFrame = false;
      canvas.classList.remove("live");
      waiting.classList.remove("hidden");
      hint.textContent = "";
      window.clearTimeout(hintTimer);
      hintTimer = window.setTimeout(() => {
        if (!gotFrame) hint.textContent = "No picture yet. Make sure CamDroid is streaming on the phone.";
      }, NO_PICTURE_HINT_MS);
      try {
        await controller.platform.api.startPreview(onFrame);
      } catch (error) {
        controller.toast.show(error instanceof Error ? error.message : String(error), "error");
      }
    },
    destroy() {
      destroyed = true;
      window.clearTimeout(hintTimer);
    },
  };
}
