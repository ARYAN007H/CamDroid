import { h } from "../core/dom";

export interface SegmentedOptions<T extends string | number> {
  label: string;
  options: ReadonlyArray<{ value: T; label: string }>;
  value: T;
  onChange: (value: T) => void;
}

export interface Segmented<T extends string | number> {
  el: HTMLElement;
  set(value: T): void;
}

/** A radio group drawn as a segmented control, with arrow-key navigation. */
export function createSegmented<T extends string | number>(config: SegmentedOptions<T>): Segmented<T> {
  const buttons = config.options.map((option) =>
    h("button", {
      attrs: { type: "button", role: "radio", "aria-checked": false, tabindex: -1 },
    }, option.label),
  );
  const el = h("div", { class: "segmented", attrs: { role: "radiogroup", "aria-label": config.label } }, ...buttons);
  let current = config.value;

  const render = (): void => {
    config.options.forEach((option, index) => {
      const selected = option.value === current;
      const button = buttons[index];
      button?.setAttribute("aria-checked", String(selected));
      button?.setAttribute("tabindex", selected ? "0" : "-1");
    });
  };
  const choose = (index: number): void => {
    const option = config.options[index];
    if (!option) return;
    current = option.value;
    render();
    buttons[index]?.focus();
    config.onChange(option.value);
  };

  buttons.forEach((button, index) => {
    button.addEventListener("click", () => choose(index));
    button.addEventListener("keydown", (event) => {
      const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
      if (step === 0) return;
      event.preventDefault();
      choose((index + step + buttons.length) % buttons.length);
    });
  });
  render();

  return {
    el,
    set(value) {
      current = value;
      render();
    },
  };
}
