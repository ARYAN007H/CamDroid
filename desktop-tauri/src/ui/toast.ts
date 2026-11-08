import { h } from "../core/dom";
import { icon } from "./icons";

export type ToastKind = "info" | "error";

export interface Toaster {
  show(message: string, kind?: ToastKind): void;
}

const VISIBLE_MS = { info: 1800, error: 4200 } as const;

/** One message at a time, announced to screen readers. */
export function createToaster(host: HTMLElement): Toaster {
  const text = h("span");
  const el = h("div", { class: "toast", attrs: { role: "status", "aria-live": "polite" } }, text);
  host.append(el);
  let timer = 0;

  return {
    show(message, kind = "info") {
      window.clearTimeout(timer);
      el.classList.toggle("error", kind === "error");
      el.replaceChildren(kind === "error" ? icon("alert", 16) : "", text);
      text.textContent = message;
      el.classList.add("visible");
      timer = window.setTimeout(() => el.classList.remove("visible"), VISIBLE_MS[kind]);
    },
  };
}
