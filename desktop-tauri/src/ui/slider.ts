import { h } from "../core/dom";

export interface SliderOptions {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  format: (value: number) => string;
  onInput: (value: number) => void;
}

export interface Slider {
  el: HTMLElement;
  /** Updates the displayed value without firing `onInput`. */
  set(value: number): void;
  setDisabled(disabled: boolean): void;
}

export function createSlider(options: SliderOptions): Slider {
  const output = h("span", { class: "value" });
  const input = h("input", {
    attrs: {
      type: "range",
      min: options.min,
      max: options.max,
      step: options.step,
      "aria-label": options.label,
    },
  });
  const el = h(
    "div",
    { class: "slider" },
    h("div", { class: "slider-head" }, h("span", { class: "name" }, options.label), output),
    input,
  );

  const render = (value: number): void => {
    input.value = String(value);
    output.textContent = options.format(value);
    const fraction = (value - options.min) / (options.max - options.min);
    el.style.setProperty("--fill", `${Math.min(1, Math.max(0, fraction)) * 100}%`);
  };

  input.addEventListener("input", () => {
    const value = Number(input.value);
    render(value);
    options.onInput(value);
  });
  render(options.value);

  return {
    el,
    set: render,
    setDisabled: (disabled) => {
      el.classList.toggle("disabled", disabled);
      input.disabled = disabled;
    },
  };
}
