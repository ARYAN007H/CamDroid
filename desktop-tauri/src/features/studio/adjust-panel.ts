import type { WhiteBalance } from "../../api/types";
import { h } from "../../core/dom";
import { formatCodec, formatCount, formatPercent, formatSigned } from "../../core/format";
import { icon } from "../../ui/icons";
import { createSegmented } from "../../ui/segmented";
import { createSlider } from "../../ui/slider";
import { createSwitchRow } from "../../ui/switch";
import { ZOOM_RANGE, type AppState } from "../shared/app-state";
import { presetName, type Controller } from "../shared/controller";

const WHITE_BALANCES: ReadonlyArray<{ mode: WhiteBalance; label: string; swatch: string }> = [
  { mode: "auto", label: "Auto", swatch: "conic-gradient(#ffb45a, #bcd3ff, #d6ffe0, #ffb45a)" },
  { mode: "daylight", label: "Day", swatch: "#fff2d0" },
  { mode: "tungsten", label: "Tungsten", swatch: "#ffb45a" },
  { mode: "fluorescent", label: "Fluor.", swatch: "#d6ffe0" },
  { mode: "cloudy", label: "Cloudy", swatch: "#bcd3ff" },
];

function group(title: string, ...content: Array<Node | null>): HTMLElement {
  return h("section", { class: "group" }, h("h3", { class: "section-title" }, title), ...content);
}

/** The right-hand drawer. Every control mirrors store state and writes through the controller. */
export function createAdjustPanel(controller: Controller): { el: HTMLElement; destroy(): void } {
  // Look -------------------------------------------------------------------------
  const lookName = h("span", { class: "section-title" });
  const strength = createSlider({
    label: "Intensity",
    min: 0,
    max: 1,
    step: 0.01,
    value: 1,
    format: formatPercent,
    onInput: (value) => controller.updateFilter({ strength: value }),
  });
  const temperature = createSlider({
    label: "Temperature",
    min: -1,
    max: 1,
    step: 0.01,
    value: 0,
    format: (value) => formatSigned(value * 100),
    onInput: (value) => controller.updateFilter({ temperature: value }),
  });
  const saturation = createSlider({
    label: "Saturation",
    min: 0,
    max: 2.5,
    step: 0.01,
    value: 1,
    format: formatPercent,
    onInput: (value) => controller.updateFilter({ saturation: value }),
  });
  const contrast = createSlider({
    label: "Contrast",
    min: 0.5,
    max: 2,
    step: 0.01,
    value: 1,
    format: formatPercent,
    onInput: (value) => controller.updateFilter({ contrast: value }),
  });
  const resetLook = h(
    "button",
    { class: "btn btn-quiet btn-small", attrs: { type: "button" }, on: { click: () => controller.resetFilter() } },
    "Reset",
  );

  // Phone camera -------------------------------------------------------------------
  const zoom = createSlider({
    label: "Zoom",
    min: ZOOM_RANGE.min,
    max: ZOOM_RANGE.max,
    step: 0.1,
    value: 1,
    format: (value) => `${value.toFixed(1)}×`,
    onInput: (value) => controller.setZoom(value),
  });
  const focusMode = createSegmented<"auto" | "manual">({
    label: "Focus mode",
    value: "auto",
    options: [
      { value: "auto", label: "Auto focus" },
      { value: "manual", label: "Manual" },
    ],
    onChange: (value) => controller.setFocusMode(value),
  });
  const focusDistance = createSlider({
    label: "Focus distance",
    min: 0,
    max: 10,
    step: 0.1,
    value: 0,
    format: (value) => (value === 0 ? "Far" : value.toFixed(1)),
    onInput: (value) => controller.setFocusDistance(value),
  });
  const exposure = createSlider({
    label: "Exposure",
    min: -4,
    max: 4,
    step: 1,
    value: 0,
    format: (value) => `${formatSigned(value)} EV`,
    onInput: (value) => controller.setExposure(value),
  });
  const wbButtons = WHITE_BALANCES.map((entry) => {
    const swatch = h("span", { class: "wb-swatch" });
    swatch.style.background = entry.swatch;
    return h(
      "button",
      {
        class: "wb-chip",
        attrs: { type: "button", role: "radio", "aria-checked": false, "aria-label": entry.label },
        on: { click: () => controller.setWhiteBalance(entry.mode) },
      },
      swatch,
      entry.label,
    );
  });
  const flash = createSwitchRow({
    title: "Flashlight",
    checked: false,
    onChange: () => controller.toggleFlash(),
  });
  const flipCamera = h(
    "button",
    {
      class: "btn btn-secondary",
      attrs: { type: "button" },
      on: { click: () => controller.switchCamera() },
    },
    icon("switchCamera", 16),
    "Switch front / back camera",
  );

  // Output -----------------------------------------------------------------------
  const virtualCamera = createSwitchRow({
    title: "Virtual camera",
    checked: false,
    onChange: () => void controller.toggleVirtualCamera(),
  });
  const mirror = createSwitchRow({
    title: "Mirror",
    hint: "Flip the picture left to right.",
    checked: false,
    onChange: () => void controller.toggleMirror(),
  });

  // Ring light --------------------------------------------------------------------
  const ringSwitch = createSwitchRow({
    title: "Ring light",
    hint: "Lights your face with a glowing screen border.",
    checked: false,
    onChange: () => void controller.toggleRingLight(),
  });
  const ringBrightness = createSlider({
    label: "Brightness",
    min: 0.1,
    max: 1,
    step: 0.01,
    value: 0.85,
    format: formatPercent,
    onInput: (value) => controller.updateRingLight({ brightness: value }),
  });
  const ringWarmth = createSlider({
    label: "Warmth",
    min: -1,
    max: 1,
    step: 0.01,
    value: 0,
    format: (value) => formatSigned(value * 100),
    onInput: (value) => controller.updateRingLight({ warmth: value }),
  });
  const ringThickness = createSlider({
    label: "Width",
    min: 0.03,
    max: 0.25,
    step: 0.005,
    value: 0.09,
    format: (value) => formatPercent(value),
    onInput: (value) => controller.updateRingLight({ thickness: value }),
  });

  // Stream -----------------------------------------------------------------------
  const stat = (label: string): { card: HTMLElement; value: HTMLElement } => {
    const value = h("div", { class: "v" }, "–");
    return { card: h("div", { class: "stat-card" }, h("div", { class: "k" }, label), value), value };
  };
  const statMode = stat("Mode");
  const statRate = stat("Frame rate");
  const statDropped = stat("Dropped");
  const statBattery = stat("Phone battery");

  const close = h(
    "button",
    {
      class: "icon-btn compact",
      attrs: { type: "button", "aria-label": "Close panel", "data-tip": "Close  I", "data-tip-below": true },
      on: { click: () => controller.togglePanel() },
    },
    icon("close", 18),
  );

  const body = h(
    "div",
    { class: "panel-body" },
    h(
      "section",
      { class: "group" },
      h("div", { class: "group-head" }, lookName, resetLook),
      strength.el,
      temperature.el,
      saturation.el,
      contrast.el,
    ),
    group(
      "Phone camera",
      zoom.el,
      focusMode.el,
      focusDistance.el,
      exposure.el,
      h("div", { class: "wb-grid", attrs: { role: "radiogroup", "aria-label": "White balance" } }, ...wbButtons),
      flash.el,
      flipCamera,
    ),
    group("Output", virtualCamera.el, mirror.el),
    group("Ring light", ringSwitch.el, ringBrightness.el, ringWarmth.el, ringThickness.el),
    group(
      "Stream",
      h("div", { class: "stat-grid" }, statMode.card, statRate.card, statDropped.card, statBattery.card),
    ),
  );

  const el = h(
    "aside",
    { class: "panel", attrs: { "aria-label": "Adjust" } },
    h("header", { class: "panel-head", attrs: { "data-tauri-drag-region": true } }, h("h2", {}, "Adjust"), close),
    body,
  );

  function sync(state: Readonly<AppState>): void {
    const { filter, outputMode, mirror: mirrored, ringLight, ringLightVisible } = state.studio;
    lookName.textContent = `Look · ${presetName(filter.preset)}`;
    strength.set(filter.strength);
    temperature.set(filter.temperature);
    saturation.set(filter.saturation);
    contrast.set(filter.contrast);

    const { camera } = state;
    zoom.set(camera.zoom);
    focusMode.set(camera.focusMode);
    focusDistance.set(camera.focusDistance);
    focusDistance.setDisabled(camera.focusMode !== "manual");
    exposure.set(camera.exposure);
    wbButtons.forEach((button, index) => {
      button.setAttribute("aria-checked", String(WHITE_BALANCES[index]?.mode === camera.whiteBalance));
    });
    flash.set(camera.flash);

    virtualCamera.set(outputMode === "virtual_camera");
    mirror.set(mirrored);
    const status = state.session?.telemetry.virtualCamera;
    if (outputMode !== "virtual_camera") {
      virtualCamera.setHint("Show up as a webcam in OBS, Zoom and browsers.");
    } else if (status?.state === "failed") {
      virtualCamera.setHint(firstLine(status.detail), true);
    } else if (status?.state === "active") {
      virtualCamera.setHint(`Sending to ${state.system.virtualCameraDevice}`);
    } else {
      virtualCamera.setHint("Starting…");
    }

    ringSwitch.set(ringLightVisible);
    ringBrightness.set(ringLight.brightness);
    ringWarmth.set(ringLight.warmth);
    ringThickness.set(ringLight.thickness);

    const session = state.session;
    if (session) {
      statMode.value.textContent = `${session.height}p ${formatCodec(session.codec)}`;
      statRate.value.textContent = `${Math.round(session.telemetry.renderFps)} / ${session.fps}`;
      statDropped.value.textContent = formatCount(session.telemetry.framesDropped);
      statBattery.value.textContent =
        session.telemetry.batteryPercent === null ? "Unknown" : `${session.telemetry.batteryPercent}%`;
    }
  }

  const unsubscribe = controller.store.subscribe(sync);
  sync(controller.store.get());
  return { el, destroy: unsubscribe };
}

/** Backend errors can carry setup hints on later lines; the panel only has room for the first. */
function firstLine(text: string): string {
  return text.split("\n")[0] ?? text;
}
