// Connection Wizard — the first page shown when the app launches.
// Three connection modes: Auto-discover, Manual WiFi, USB.

import { invoke } from "@tauri-apps/api/core";

interface DeviceInfo {
  name: string;
  ip: string;
  port: number;
  model: string;
  codecs: string;
  resolution: string;
}

interface ConnectionPageCallbacks {
  onConnected: () => void;
}

type ConnectionState = "idle" | "scanning" | "found" | "connecting" | "error";

let state: ConnectionState = "idle";
let discoveredDevices: DeviceInfo[] = [];
let errorMessage = "";

export function renderConnectionPage(
  container: HTMLElement,
  callbacks: ConnectionPageCallbacks
) {
  container.innerHTML = buildConnectionHTML();
  attachConnectionEvents(container, callbacks);
}

function buildConnectionHTML(): string {
  return `
    <div class="connection-page animate-fade-in">
      <div class="logo">
        <div class="logo-icon">📹</div>
        <h1>CamDroid</h1>
      </div>
      <p class="subtitle">
        Connect your Android phone to use it as a virtual webcam on your computer
      </p>

      <div id="connection-content">
        ${buildConnectionCards()}
      </div>
    </div>
  `;
}

function buildConnectionCards(): string {
  return `
    <div class="connection-grid animate-slide-up">
      <div class="card interactive connection-card" id="btn-discover">
        <div class="icon">📡</div>
        <h3>Auto-Discover</h3>
        <p>Find your phone automatically on the same WiFi network</p>
      </div>
      <div class="card interactive connection-card" id="btn-manual">
        <div class="icon">🌐</div>
        <h3>Manual IP</h3>
        <p>Enter your phone's IP address and port directly</p>
      </div>
      <div class="card interactive connection-card" id="btn-usb">
        <div class="icon">🔌</div>
        <h3>USB (ADB)</h3>
        <p>Connect via USB cable for the lowest latency</p>
      </div>
    </div>
  `;
}

function buildScanningHTML(): string {
  return `
    <div class="scanning-overlay animate-scale-in">
      <div class="scanning-ring"></div>
      <h2>Searching for devices...</h2>
      <p style="color: var(--text-secondary); font-size: 0.85rem;">
        Make sure CamDroid is running on your phone
      </p>
      <button class="btn btn-secondary" id="btn-cancel-scan">Cancel</button>
    </div>
  `;
}

function buildDeviceListHTML(devices: DeviceInfo[]): string {
  if (devices.length === 0) {
    return `
      <div class="scanning-overlay animate-scale-in">
        <div style="font-size: 3rem; opacity: 0.5;">📱</div>
        <h2>No Devices Found</h2>
        <p style="color: var(--text-secondary); font-size: 0.85rem; text-align: center; max-width: 320px;">
          Make sure the CamDroid app is running and your phone is on the same WiFi network
        </p>
        <div class="flex gap-sm">
          <button class="btn btn-primary" id="btn-retry-scan">Retry</button>
          <button class="btn btn-secondary" id="btn-back-home">Back</button>
        </div>
      </div>
    `;
  }

  const deviceItems = devices
    .map(
      (d, i) => `
      <div class="card interactive device-item animate-slide-up" data-device-index="${i}" style="animation-delay: ${i * 80}ms">
        <div class="device-icon">📱</div>
        <div class="device-info">
          <div class="device-name">${d.model}</div>
          <div class="device-addr">${d.ip}:${d.port}</div>
        </div>
        <span class="badge badge-green"><span class="dot"></span>Ready</span>
      </div>
    `
    )
    .join("");

  return `
    <div class="flex flex-col items-center gap-lg animate-fade-in">
      <h2>Found ${devices.length} device${devices.length > 1 ? "s" : ""}</h2>
      <div class="device-list">${deviceItems}</div>
      <button class="btn btn-secondary" id="btn-back-home">Back</button>
    </div>
  `;
}

function buildManualFormHTML(): string {
  return `
    <div class="flex flex-col items-center gap-lg animate-scale-in" style="width: 100%;">
      <h2>Connect Manually</h2>
      <p style="color: var(--text-secondary); font-size: 0.85rem;">
        Enter the IP address shown in the CamDroid app
      </p>
      <div class="ip-form">
        <input type="text" id="input-ip" placeholder="192.168.1.100" autofocus />
        <input type="number" id="input-port" class="port-input" placeholder="4747" value="4747" />
        <button class="btn btn-primary" id="btn-connect-manual">Connect</button>
      </div>
      <button class="btn btn-secondary" id="btn-back-home">Back</button>
    </div>
  `;
}

function buildConnectingHTML(label: string): string {
  return `
    <div class="scanning-overlay animate-scale-in">
      <div class="spinner" style="width: 40px; height: 40px;"></div>
      <h2>Connecting...</h2>
      <p style="color: var(--text-secondary); font-size: 0.85rem;">${label}</p>
    </div>
  `;
}

function buildErrorHTML(msg: string): string {
  return `
    <div class="scanning-overlay animate-scale-in">
      <div style="font-size: 3rem;">⚠️</div>
      <h2>Connection Failed</h2>
      <p style="color: var(--red); font-size: 0.85rem; text-align: center; max-width: 400px;">${msg}</p>
      <button class="btn btn-primary" id="btn-back-home">Try Again</button>
    </div>
  `;
}

function attachConnectionEvents(
  container: HTMLElement,
  callbacks: ConnectionPageCallbacks
) {
  const content = container.querySelector("#connection-content") as HTMLElement;
  if (!content) return;

  // Auto-discover
  content.querySelector("#btn-discover")?.addEventListener("click", async () => {
    content.innerHTML = buildScanningHTML();
    attachCancelScan(content, callbacks);

    try {
      const devices = await invoke<DeviceInfo[]>("discover_devices", { timeout: 5 });
      discoveredDevices = devices;
      content.innerHTML = buildDeviceListHTML(devices);
      attachDeviceListEvents(content, callbacks);
    } catch (err) {
      content.innerHTML = buildErrorHTML(String(err));
      attachBackButton(content, callbacks);
    }
  });

  // Manual IP
  content.querySelector("#btn-manual")?.addEventListener("click", () => {
    content.innerHTML = buildManualFormHTML();
    attachManualFormEvents(content, callbacks);
  });

  // USB
  content.querySelector("#btn-usb")?.addEventListener("click", async () => {
    content.innerHTML = buildConnectingHTML("Setting up USB connection via ADB...");

    try {
      await invoke("connect_and_stream", {
        params: {
          mode: "usb",
          codec: "h264",
          resolution: "1080p",
          fps: 60,
          audio: true,
        },
      });
      callbacks.onConnected();
    } catch (err) {
      content.innerHTML = buildErrorHTML(String(err));
      attachBackButton(content, callbacks);
    }
  });
}

function attachCancelScan(
  content: HTMLElement,
  callbacks: ConnectionPageCallbacks
) {
  content.querySelector("#btn-cancel-scan")?.addEventListener("click", () => {
    content.innerHTML = buildConnectionCards();
    attachConnectionEvents(content.parentElement!, callbacks);
  });
}

function attachDeviceListEvents(
  content: HTMLElement,
  callbacks: ConnectionPageCallbacks
) {
  // Click on a discovered device
  content.querySelectorAll("[data-device-index]").forEach((el) => {
    el.addEventListener("click", async () => {
      const idx = parseInt(el.getAttribute("data-device-index") || "0");
      const device = discoveredDevices[idx];
      if (!device) return;

      content.innerHTML = buildConnectingHTML(
        `${device.model} at ${device.ip}:${device.port}`
      );

      try {
        await invoke("connect_and_stream", {
          params: {
            mode: "wifi",
            ip: device.ip,
            port: device.port,
            codec: "h264",
            resolution: "1080p",
            fps: 60,
            audio: true,
          },
        });
        callbacks.onConnected();
      } catch (err) {
        content.innerHTML = buildErrorHTML(String(err));
        attachBackButton(content, callbacks);
      }
    });
  });

  // Retry scan
  content.querySelector("#btn-retry-scan")?.addEventListener("click", () => {
    // Re-trigger discover
    content.parentElement
      ?.querySelector("#btn-discover")
      ?.dispatchEvent(new Event("click"));
  });

  attachBackButton(content, callbacks);
}

function attachManualFormEvents(
  content: HTMLElement,
  callbacks: ConnectionPageCallbacks
) {
  const connectBtn = content.querySelector("#btn-connect-manual");
  const ipInput = content.querySelector("#input-ip") as HTMLInputElement;
  const portInput = content.querySelector("#input-port") as HTMLInputElement;

  const doConnect = async () => {
    const ip = ipInput?.value.trim();
    const port = parseInt(portInput?.value || "4747");

    if (!ip) {
      ipInput?.focus();
      return;
    }

    content.innerHTML = buildConnectingHTML(`${ip}:${port}`);

    try {
      await invoke("connect_and_stream", {
        params: {
          mode: "wifi",
          ip,
          port,
          codec: "h264",
          resolution: "1080p",
          fps: 60,
          audio: true,
        },
      });
      callbacks.onConnected();
    } catch (err) {
      content.innerHTML = buildErrorHTML(String(err));
      attachBackButton(content, callbacks);
    }
  };

  connectBtn?.addEventListener("click", doConnect);

  // Enter key to connect
  ipInput?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") doConnect();
  });
  portInput?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") doConnect();
  });

  attachBackButton(content, callbacks);
}

function attachBackButton(
  content: HTMLElement,
  callbacks: ConnectionPageCallbacks
) {
  content.querySelector("#btn-back-home")?.addEventListener("click", () => {
    renderConnectionPage(content.closest("#app") || content, callbacks);
  });
}
