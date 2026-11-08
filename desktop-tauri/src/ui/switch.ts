import { h } from "../core/dom";

export interface SwitchRowOptions {
  title: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

export interface SwitchRow {
  el: HTMLElement;
  set(checked: boolean): void;
  /** Replaces the helper text; `problem` tints it as a warning. */
  setHint(text: string, problem?: boolean): void;
}

export function createSwitchRow(options: SwitchRowOptions): SwitchRow {
  const hint = h("div", { class: "hint" }, options.hint);
  const control = h("button", {
    class: "switch",
    attrs: { type: "button", role: "switch", "aria-label": options.title, "aria-checked": options.checked },
  });
  const el = h(
    "div",
    { class: "switch-row" },
    h("div", { class: "label" }, h("div", { class: "title" }, options.title), hint),
    control,
  );

  const set = (checked: boolean): void => control.setAttribute("aria-checked", String(checked));
  control.addEventListener("click", () => {
    const next = control.getAttribute("aria-checked") !== "true";
    set(next);
    options.onChange(next);
  });
  hint.hidden = !options.hint;

  return {
    el,
    set,
    setHint(text, problem = false) {
      hint.textContent = text;
      hint.hidden = text === "";
      hint.classList.toggle("problem", problem);
    },
  };
}
