// Dashboard — main view after connecting to a phone.
// Shows live preview placeholder, camera controls sidebar, and stream stats.

import { invoke } from "@tauri-apps/api/core";

interface DashboardCallbacks {
  onDisconnected: () => void;
}

export function renderDashboard(
  container: HTMLElement,
  callbacks: DashboardCallbacks
) {
  container.innerHTML = buildDashboardHTML();
  attachDashboardEvents(container, callbacks);
  startStatusPolling(container);
}

function buildDashboardHTML(): string {
  return `
    <div class="dashboard">
      <!-- Header -->
      <div class="dashboard-header">
        <div class="header-left">
          <div class="app-title">
            <span>📹</span>
            <span>CamDroid</span>
          </div>
          <span class="badge badge-green" id="status-badge">
            <span class="dot pulse"></span>
            Connected
          </span>
        </div>
        <div class="header-right">
          <div class="stat" id="stat-fps">
            <span class="value">--</span>
            <span class="unit">FPS</span>
          </div>
          <div class="stat" id="stat-bitrate">
            <span class="value">--</span>
            <span class="unit">Mbps</span>
          </div>
          <div class="stat" id="stat-battery">
            <span class="value">--</span>
            <span class="unit">🔋</span>
          </div>
          <button class="btn btn-danger" id="btn-disconnect">Disconnect</button>
        </div>
      </div>

      <!-- Preview Area -->
      <div class="preview-area">
        <div class="preview-placeholder" id="preview-content">
          <div class="icon">📹</div>
          <h2>Live Preview</h2>
          <p>Video is streaming to your virtual webcam</p>
          <p style="font-size: 0.75rem; color: var(--text-dim);">
            Open Zoom, OBS, or any video app — select "CamDroid" as your camera
          </p>
        </div>
      </div>

      <!-- Controls Sidebar -->
      <div class="controls-sidebar">
        ${buildControlsHTML()}
      </div>

      <!-- Stream Info Bar -->
      <div class="stream-bar">
        <div class="stream-info">
          <span class="badge badge-neutral" id="info-codec">H.264</span>
          <span class="badge badge-neutral" id="info-resolution">1080p</span>
          <span class="badge badge-neutral" id="info-fps-target">60 FPS</span>
          <span class="badge badge-neutral" id="info-audio">🔊 Audio ON</span>
        </div>
        <div class="flex gap-sm items-center">
          <span style="font-size: 0.75rem; color: var(--text-dim);" id="device-name">Unknown Device</span>
        </div>
      </div>
    </div>
  `;
}

function buildControlsHTML(): string {
  return `
    <!-- Zoom -->
    <div class="control-group">
      <div class="section-header">ZOOM</div>
      <div class="control-row">
        <span class="label">Level</span>
        <input type="range" id="ctrl-zoom" min="1" max="10" step="0.1" value="1" />
        <span class="value" id="val-zoom">1.0x</span>
      </div>
    </div>

    <div class="separator"></div>

    <!-- Focus -->
    <div class="control-group">
      <div class="section-header">FOCUS</div>
      <div class="control-buttons" style="grid-template-columns: 1fr 1fr;">
        <button class="btn btn-icon active" id="btn-focus-auto" title="Auto Focus">AF</button>
        <button class="btn btn-icon" id="btn-focus-manual" title="Manual Focus">MF</button>
      </div>
      <div class="control-row" id="focus-distance-row" style="display: none;">
        <span class="label">Distance</span>
        <input type="range" id="ctrl-focus" min="0" max="10" step="0.1" value="0" />
        <span class="value" id="val-focus">0.0</span>
      </div>
    </div>

    <div class="separator"></div>

    <!-- Exposure -->
    <div class="control-group">
      <div class="section-header">EXPOSURE</div>
      <div class="control-row">
        <span class="label">EV</span>
        <input type="range" id="ctrl-exposure" min="-4" max="4" step="1" value="0" />
        <span class="value" id="val-exposure">0</span>
      </div>
    </div>

    <div class="separator"></div>

    <!-- White Balance -->
    <div class="control-group">
      <div class="section-header">WHITE BALANCE</div>
      <div class="control-buttons" style="grid-template-columns: repeat(5, 1fr);">
        <button class="btn btn-icon active" data-wb="auto" title="Auto">A</button>
        <button class="btn btn-icon" data-wb="daylight" title="Daylight">☀️</button>
        <button class="btn btn-icon" data-wb="tungsten" title="Tungsten">💡</button>
        <button class="btn btn-icon" data-wb="fluorescent" title="Fluorescent">🔬</button>
        <button class="btn btn-icon" data-wb="cloudy" title="Cloudy">☁️</button>
      </div>
    </div>

    <div class="separator"></div>

    <!-- Quick Actions -->
    <div class="control-group">
      <div class="section-header">ACTIONS</div>
      <div class="control-buttons">
        <button class="btn btn-icon" id="btn-flash" title="Flash">⚡</button>
        <button class="btn btn-icon" id="btn-flip" title="Switch Camera">🔄</button>
        <button class="btn btn-icon" id="btn-mirror" title="Mirror">🪞</button>
      </div>
    </div>

    <div class="separator"></div>

    <!-- Stream Config -->
    <div class="control-group">
      <div class="section-header">STREAM CONFIG</div>
      <div class="control-row">
        <span class="label">Codec</span>
        <select id="ctrl-codec" style="background: var(--surface-2); border: 1px solid var(--outline); border-radius: var(--radius-sm); padding: 6px 10px; color: var(--text-primary); font-family: var(--font-mono); font-size: 0.8rem; outline: none;">
          <option value="h264">H.264</option>
          <option value="h265">H.265</option>
          <option value="mjpeg">MJPEG</option>
        </select>
      </div>
      <div class="control-row">
        <span class="label">Resolution</span>
        <select id="ctrl-resolution" style="background: var(--surface-2); border: 1px solid var(--outline); border-radius: var(--radius-sm); padding: 6px 10px; color: var(--text-primary); font-family: var(--font-mono); font-size: 0.8rem; outline: none;">
          <option value="1080p">1080p</option>
          <option value="1440p">1440p</option>
          <option value="4k">4K</option>
        </select>
      </div>
    </div>
  `;
}

// ── Event Handlers ──

function attachDashboardEvents(
  container: HTMLElement,
  callbacks: DashboardCallbacks
) {
  // Disconnect button
  container.querySelector("#btn-disconnect")?.addEventListener("click", async () => {
    try {
      await invoke("stop_stream");
    } catch (err) {
      console.error("Stop error:", err);
    }
    callbacks.onDisconnected();
  });

  // Zoom slider
  const zoomSlider = container.querySelector("#ctrl-zoom") as HTMLInputElement;
  const zoomVal = container.querySelector("#val-zoom") as HTMLElement;
  zoomSlider?.addEventListener("input", () => {
    const v = parseFloat(zoomSlider.value);
    zoomVal.textContent = `${v.toFixed(1)}x`;
  });
  zoomSlider?.addEventListener("change", async () => {
    const v = parseFloat(zoomSlider.value);
    try {
      await invoke("set_zoom", { value: v });
    } catch (err) {
      console.error("Zoom error:", err);
    }
  });

  // Focus mode buttons
  const focusAuto = container.querySelector("#btn-focus-auto") as HTMLElement;
  const focusManual = container.querySelector("#btn-focus-manual") as HTMLElement;
  const focusDistRow = container.querySelector("#focus-distance-row") as HTMLElement;

  focusAuto?.addEventListener("click", async () => {
    focusAuto.classList.add("active");
    focusManual?.classList.remove("active");
    if (focusDistRow) focusDistRow.style.display = "none";
    try {
      await invoke("set_focus", { mode: "auto", distance: null });
    } catch (err) {
      console.error("Focus error:", err);
    }
  });

  focusManual?.addEventListener("click", () => {
    focusManual.classList.add("active");
    focusAuto?.classList.remove("active");
    if (focusDistRow) focusDistRow.style.display = "flex";
  });

  // Focus distance slider
  const focusSlider = container.querySelector("#ctrl-focus") as HTMLInputElement;
  const focusVal = container.querySelector("#val-focus") as HTMLElement;
  focusSlider?.addEventListener("input", () => {
    focusVal.textContent = parseFloat(focusSlider.value).toFixed(1);
  });
  focusSlider?.addEventListener("change", async () => {
    try {
      await invoke("set_focus", {
        mode: "manual",
        distance: parseFloat(focusSlider.value),
      });
    } catch (err) {
      console.error("Focus error:", err);
    }
  });

  // Exposure slider
  const expSlider = container.querySelector("#ctrl-exposure") as HTMLInputElement;
  const expVal = container.querySelector("#val-exposure") as HTMLElement;
  expSlider?.addEventListener("input", () => {
    const v = parseInt(expSlider.value);
    expVal.textContent = v > 0 ? `+${v}` : `${v}`;
  });
  expSlider?.addEventListener("change", async () => {
    try {
      await invoke("set_exposure", { compensation: parseInt(expSlider.value) });
    } catch (err) {
      console.error("Exposure error:", err);
    }
  });

  // White balance buttons
  container.querySelectorAll("[data-wb]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      container.querySelectorAll("[data-wb]").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      const mode = btn.getAttribute("data-wb") || "auto";
      try {
        await invoke("set_white_balance", { mode });
      } catch (err) {
        console.error("WB error:", err);
      }
    });
  });

  // Flash toggle
  let flashOn = false;
  container.querySelector("#btn-flash")?.addEventListener("click", async () => {
    flashOn = !flashOn;
    container.querySelector("#btn-flash")?.classList.toggle("active", flashOn);
    try {
      await invoke("set_flash", { enabled: flashOn });
    } catch (err) {
      console.error("Flash error:", err);
    }
  });

  // Camera switch
  container.querySelector("#btn-flip")?.addEventListener("click", async () => {
    try {
      await invoke("switch_camera");
    } catch (err) {
      console.error("Switch error:", err);
    }
  });

  // Mirror toggle
  let mirrorOn = false;
  container.querySelector("#btn-mirror")?.addEventListener("click", async () => {
    mirrorOn = !mirrorOn;
    container.querySelector("#btn-mirror")?.classList.toggle("active", mirrorOn);
    try {
      await invoke("send_control", {
        command: JSON.stringify({ cmd: "set_mirror", enabled: mirrorOn }),
      });
    } catch (err) {
      console.error("Mirror error:", err);
    }
  });

  // Codec change
  container.querySelector("#ctrl-codec")?.addEventListener("change", async (e) => {
    const value = (e.target as HTMLSelectElement).value;
    try {
      await invoke("send_control", {
        command: JSON.stringify({ cmd: "set_codec", value }),
      });
      const badge = container.querySelector("#info-codec");
      if (badge) badge.textContent = value.toUpperCase();
    } catch (err) {
      console.error("Codec error:", err);
    }
  });

  // Resolution change
  container.querySelector("#ctrl-resolution")?.addEventListener("change", async (e) => {
    const value = (e.target as HTMLSelectElement).value;
    try {
      await invoke("send_control", {
        command: JSON.stringify({ cmd: "set_resolution", value }),
      });
      const badge = container.querySelector("#info-resolution");
      if (badge) badge.textContent = value.toUpperCase();
    } catch (err) {
      console.error("Resolution error:", err);
    }
  });
}

// ── Status Polling ──

let statusInterval: number | null = null;

function startStatusPolling(container: HTMLElement) {
  if (statusInterval) clearInterval(statusInterval);

  statusInterval = window.setInterval(async () => {
    try {
      const status = await invoke<{
        connected: boolean;
        streaming: boolean;
        device_name: string;
        codec: string;
        resolution: string;
        fps: number;
        battery: number;
      }>("get_status");

      // Update stats
      const batteryEl = container.querySelector("#stat-battery .value");
      if (batteryEl) batteryEl.textContent = `${status.battery}%`;

      const deviceEl = container.querySelector("#device-name");
      if (deviceEl) deviceEl.textContent = status.device_name;

      // Update badges
      const statusBadge = container.querySelector("#status-badge");
      if (statusBadge && !status.connected) {
        statusBadge.className = "badge badge-red";
        statusBadge.innerHTML = `<span class="dot"></span>Disconnected`;
      }
    } catch {
      // Session ended — stop polling
      if (statusInterval) clearInterval(statusInterval);
    }
  }, 2000);
}

export function stopStatusPolling() {
  if (statusInterval) {
    clearInterval(statusInterval);
    statusInterval = null;
  }
}
