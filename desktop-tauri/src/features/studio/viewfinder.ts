import { h } from "../../core/dom";
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
        if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
          canvas.width = bitmap.width;
          canvas.height = bitmap.height;
        }
        context?.drawImage(bitmap, 0, 0);
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
