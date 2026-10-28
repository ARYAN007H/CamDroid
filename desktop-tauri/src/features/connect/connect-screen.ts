import type { DeviceInfo, StreamPreferences } from "../../api/types";
import { h, replaceChildren } from "../../core/dom";
import { errorMessage, formatCodec } from "../../core/format";
import { comboLabels } from "../../core/keyboard";
import { icon, type IconName } from "../../ui/icons";
import { createSegmented } from "../../ui/segmented";
import { createSwitchRow } from "../../ui/switch";
import type { AppState } from "../shared/app-state";
import type { Controller } from "../shared/controller";
import type { Screen } from "../shared/screen";

type Method = "discover" | "wifi" | "usb";

interface MethodInfo {
  id: Method;
  title: string;
  subtitle: string;
  icon: IconName;
}

const METHODS: readonly MethodInfo[] = [
  { id: "discover", title: "Find my phone", subtitle: "On the same Wi-Fi network", icon: "search" },
  { id: "wifi", title: "Enter an address", subtitle: "Type the IP shown in the app", icon: "globe" },
  { id: "usb", title: "USB cable", subtitle: "Lowest latency, most reliable", icon: "usb" },
];

type ScanState =
  | { phase: "idle" }
  | { phase: "scanning" }
  | { phase: "done"; devices: DeviceInfo[] }
  | { phase: "failed"; message: string };

const SCAN_SECONDS = 4;

export function createConnectScreen(controller: Controller): Screen {
  const { store, platform } = controller;
  let method: Method = "discover";
  let scan: ScanState = { phase: "idle" };
  let qualityOpen = false;
  let destroyed = false;

  const methodButtons = new Map<Method, HTMLButtonElement>();
  const methodDetails = new Map<Method, HTMLElement>();

  // Discover ------------------------------------------------------------------

  const discoverBody = h("div", { class: "method-detail", attrs: { id: "detail-discover" } });

  async function startScan(): Promise<void> {
    scan = { phase: "scanning" };
    renderDiscover();
    try {
      const devices = await platform.api.discoverDevices(SCAN_SECONDS);
      if (!destroyed) scan = { phase: "done", devices };
    } catch (error) {
      if (!destroyed) scan = { phase: "failed", message: errorMessage(error) };
    }
    if (!destroyed) renderDiscover();
  }

  function connectToDevice(device: DeviceInfo): void {
    void controller.connect(
      {
        target: { mode: "wifi", host: device.ip, port: device.port },
        stream: store.get().studio.stream,
      },
      device.model,
    );
  }

  function renderDiscover(): void {
    const rescan = h(
      "button",
      { class: "btn btn-quiet btn-small", attrs: { type: "button" }, on: { click: () => void startScan() } },
      icon("refresh", 14),
      "Scan again",
    );

    if (scan.phase === "scanning" || scan.phase === "idle") {
      replaceChildren(
        discoverBody,
        h("div", { class: "scan-state" }, h("div", { class: "spinner small" }), "Looking for phones…"),
      );
      return;
    }
    if (scan.phase === "failed") {
      replaceChildren(
        discoverBody,
        h("div", { class: "inline-error" }, scan.message),
        h("div", { class: "detail-actions" }, rescan),
      );
      return;
    }
    if (scan.devices.length === 0) {
      replaceChildren(
        discoverBody,
        h("p", { class: "empty-state" }, "No phones found. Open CamDroid on your phone and tap Start."),
        h("p", { class: "note" }, "If your router blocks device discovery, enter the address instead."),
        h("div", { class: "detail-actions" }, rescan),
      );
      return;
    }
    replaceChildren(
      discoverBody,
      h(
        "div",
        { class: "device-list" },
        ...scan.devices.map((device) =>
          h(
            "button",
            { class: "device", attrs: { type: "button" }, on: { click: () => connectToDevice(device) } },
            icon("phone", 20),
            h(
              "span",
              { class: "info" },
              h("div", { class: "name" }, device.model),
              h("div", { class: "addr" }, `${device.ip}:${device.port}`),
            ),
            h("span", { class: "go" }, icon("chevronRight", 18)),
          ),
        ),
      ),
      h("div", { class: "detail-actions" }, rescan),
    );
  }

  // Manual address ---------------------------------------------------------------

  const initialWifi = store.get().studio.wifi;
  const hostField = h("input", {
    class: "field",
    attrs: {
      type: "text",
      placeholder: "192.168.1.67",
      value: initialWifi.host,
      "aria-label": "Phone IP address",
      autocomplete: "off",
      spellcheck: false,
    },
  });
  const portField = h("input", {
    class: "field port",
    attrs: {
      type: "number",
      min: 1,
      max: 65535,
      value: initialWifi.port,
      "aria-label": "Port",
    },
  });

  function connectManually(): void {
    const host = hostField.value.trim();
    const port = Number(portField.value);
    const validPort = Number.isInteger(port) && port >= 1 && port <= 65535;
    hostField.setAttribute("aria-invalid", String(host === ""));
    portField.setAttribute("aria-invalid", String(!validPort));
    if (host === "") {
      hostField.focus();
      return;
    }
    if (!validPort) {
      portField.focus();
      return;
    }
    void controller.connect(
      { target: { mode: "wifi", host, port }, stream: store.get().studio.stream },
      `${host}:${port}`,
    );
  }
  for (const field of [hostField, portField]) {
    field.addEventListener("keydown", (event) => {
      if (event.key === "Enter") connectManually();
    });
  }

  const wifiBody = h(
    "div",
    { class: "method-detail", attrs: { id: "detail-wifi" } },
    h("div", { class: "manual-form" }, hostField, portField),
    h(
      "div",
      { class: "detail-actions" },
      h("span", { class: "note", attrs: { style: "margin:0" } }, "Shown on the phone's home screen."),
      h("button", { class: "btn btn-primary", attrs: { type: "button" }, on: { click: connectManually } }, "Connect"),
    ),
  );

  // USB ----------------------------------------------------------------------------

  const usbNote = h("p", { class: "note", attrs: { style: "margin-top:0" } });
  const usbButton = h(
    "button",
    {
      class: "btn btn-primary",
      attrs: { type: "button" },
      on: {
        click: () =>
          void controller.connect({ target: { mode: "usb" }, stream: store.get().studio.stream }, "your phone (USB)"),
      },
    },
    "Connect",
  );
  const usbBody = h(
    "div",
    { class: "method-detail", attrs: { id: "detail-usb" } },
    h("p", { class: "empty-state" }, "Plug in your phone, enable USB debugging, and tap Start in CamDroid."),
    usbNote,
    h("div", { class: "detail-actions" }, h("span"), usbButton),
  );

  // Method list ------------------------------------------------------------------

  methodDetails.set("discover", discoverBody);
  methodDetails.set("wifi", wifiBody);
  methodDetails.set("usb", usbBody);

  function selectMethod(next: Method, focus = false): void {
    method = next;
    for (const info of METHODS) {
      const selected = info.id === method;
      methodButtons.get(info.id)?.setAttribute("aria-checked", String(selected));
      methodButtons.get(info.id)?.setAttribute("tabindex", selected ? "0" : "-1");
      const detail = methodDetails.get(info.id);
      if (detail) detail.hidden = !selected;
    }
    if (focus) methodButtons.get(next)?.focus();
    if (next === "discover" && scan.phase === "idle") void startScan();
    if (next === "wifi") hostField.focus();
  }

  const methodItems = METHODS.map((info, index) => {
    const button = h(
      "button",
      {
        class: "method",
        attrs: { type: "button", role: "radio", "aria-checked": false, "aria-controls": `detail-${info.id}` },
        on: {
          click: () => selectMethod(info.id),
          keydown: (event) => {
            const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
            if (step === 0) return;
            event.preventDefault();
            const next = METHODS[(index + step + METHODS.length) % METHODS.length];
            if (next) selectMethod(next.id, true);
          },
        },
      },
      h("span", { class: "method-icon" }, icon(info.icon, 20)),
      h("span", { class: "method-text" }, h("span", { class: "method-title" }, info.title), h("span", { class: "method-sub" }, info.subtitle)),
      h("span", { class: "method-radio" }),
    );
    methodButtons.set(info.id, button);
    return h("div", { class: "method-item" }, button, methodDetails.get(info.id));
  });
  const methodsEl = h("div", { class: "methods", attrs: { role: "radiogroup", "aria-label": "Connection method" } }, ...methodItems);

  // Stream quality --------------------------------------------------------------

  const qualityValue = h("span");
  const qualityChevron = h("span", { class: "chevron" }, icon("chevronRight", 16));
  const qualitySummary = h(
    "button",
    {
      class: "quality-summary",
      attrs: { type: "button", "aria-expanded": false },
      on: {
        click: () => {
          qualityOpen = !qualityOpen;
          renderQuality();
        },
      },
    },
    h("span", { class: "label" }, "Stream quality"),
    h("span", { class: "value" }, qualityValue, qualityChevron),
  );

  const stream = store.get().studio.stream;
  const resolution = createSegmented<StreamPreferences["resolution"]>({
    label: "Resolution",
    value: stream.resolution,
    options: [
      { value: "1080p", label: "1080p" },
      { value: "1440p", label: "1440p" },
      { value: "4k", label: "4K" },
    ],
    onChange: (value) => controller.updateStreamPreferences({ resolution: value }),
  });
  const frameRate = createSegmented<StreamPreferences["fps"]>({
    label: "Frame rate",
    value: stream.fps,
    options: [
      { value: 30, label: "30 fps" },
      { value: 60, label: "60 fps" },
    ],
    onChange: (value) => controller.updateStreamPreferences({ fps: value }),
  });
  const codec = createSegmented<StreamPreferences["codec"]>({
    label: "Codec",
    value: stream.codec,
    options: [
      { value: "h264", label: "H.264" },
      { value: "h265", label: "H.265" },
      { value: "mjpeg", label: "MJPEG" },
    ],
    onChange: (value) => controller.updateStreamPreferences({ codec: value }),
  });
  const audio = createSwitchRow({
    title: "Microphone",
    hint: "Send the phone's audio as a virtual microphone.",
    checked: stream.audio,
    onChange: (checked) => controller.updateStreamPreferences({ audio: checked }),
  });
  const qualityBody = h(
    "div",
    { class: "quality-body" },
    h("div", { class: "quality-row" }, h("span", { class: "name" }, "Resolution"), resolution.el),
    h("div", { class: "quality-row" }, h("span", { class: "name" }, "Frame rate"), frameRate.el),
    h("div", { class: "quality-row" }, h("span", { class: "name" }, "Codec"), codec.el),
    audio.el,
    h(
      "p",
      { class: "note", attrs: { style: "margin:0" } },
      "The phone may offer a different mode; CamDroid uses the closest one.",
    ),
  );

  function renderQuality(): void {
    const { stream: current } = store.get().studio;
    const resolutionLabel = current.resolution === "4k" ? "4K" : current.resolution;
    qualityValue.textContent = `${resolutionLabel} · ${current.fps} fps · ${formatCodec(current.codec)}`;
    qualitySummary.setAttribute("aria-expanded", String(qualityOpen));
    qualityBody.hidden = !qualityOpen;
  }

  // Footer -----------------------------------------------------------------------

  const readinessDot = h("span", { class: "dot" });
  const readinessText = h("span");
  const footer = h(
    "footer",
    { class: "connect-footer" },
    h("div", { class: "readiness" }, readinessDot, readinessText),
    h(
      "div",
      { class: "hints" },
      hint(["ctrl+q"], "Quit"),
      hint(["?"], "Shortcuts"),
    ),
  );

  // Layout -----------------------------------------------------------------------

  const errorEl = h("div", { class: "inline-error", attrs: { role: "alert" } });
  const form = h("div", { attrs: { style: "display:flex;flex-direction:column;gap:22px" } }, methodsEl, errorEl, h("div", { class: "quality" }, qualitySummary, qualityBody), footer);
  const connecting = h("div", { class: "connecting", attrs: { role: "status" } });
  const card = h(
    "div",
    { class: "connect-card" },
    h(
      "header",
      { class: "brand" },
      h("img", { class: "brand-mark", attrs: { src: "/app-icon.png", alt: "", draggable: false } }),
      h("div", {}, h("h1", {}, "CamDroid"), h("p", {}, "Use your Android phone as a webcam")),
    ),
    form,
    connecting,
  );
  const el = h(
    "section",
    { class: "connect" },
    h("div", { class: "connect-drag", attrs: { "data-tauri-drag-region": true } }),
    card,
  );

  function sync(state: Readonly<AppState>): void {
    const busy = state.connecting !== null;
    form.hidden = busy;
    connecting.hidden = !busy;
    if (state.connecting) {
      replaceChildren(
        connecting,
        h("div", { class: "spinner" }),
        h("h2", {}, "Connecting…"),
        h("p", {}, `Reaching ${state.connecting.label}`),
        h(
          "button",
          { class: "btn btn-secondary", attrs: { type: "button" }, on: { click: () => controller.cancelConnect() } },
          "Cancel",
          h("span", { class: "kbd" }, "Esc"),
        ),
      );
    }

    errorEl.hidden = state.connectError === null;
    errorEl.textContent = state.connectError ?? "";

    renderQuality();

    const { system } = state;
    readinessDot.className = `dot ${system.virtualCameraReady ? "ok" : "warn"}`;
    readinessText.textContent = system.virtualCameraReady
      ? `Virtual camera ready (${system.virtualCameraDevice})`
      : "Virtual camera not set up. Run desktop/setup.sh";

    usbButton.disabled = !system.adbAvailable;
    usbNote.hidden = system.adbAvailable;
    usbNote.className = "note problem";
    usbNote.textContent = "adb was not found. Install android-tools (Arch) or android-tools-adb (Debian/Ubuntu).";
  }

  const unsubscribe = store.subscribe(sync);
  renderDiscover();
  selectMethod("discover");
  sync(store.get());

  return {
    el,
    focus: () => methodButtons.get(method)?.focus(),
    destroy() {
      destroyed = true;
      unsubscribe();
    },
  };
}

function hint(combo: string[], label: string): HTMLElement {
  return h(
    "span",
    { class: "hint-item" },
    ...combo.flatMap((keys) => comboLabels(keys).map((key) => h("span", { class: "kbd" }, key))),
    label,
  );
}
