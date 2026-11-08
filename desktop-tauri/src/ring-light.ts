import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./styles/tokens.css";
import "./styles/ring-light.css";
import type { RingLightSettings, StudioState } from "./api/types";
import { clamp } from "./core/format";
import { ringLightColor } from "./core/ring-light-color";

const ring = document.getElementById("ring");
if (!ring) throw new Error("#ring is missing from ring-light.html");

/** Paints the ring. Brightness drives opacity; thickness is a share of the shorter screen side. */
function paint(settings: RingLightSettings): void {
  const shorter = Math.min(window.innerWidth, window.innerHeight);
  ring!.style.setProperty("--ring-color", ringLightColor(settings.warmth));
  ring!.style.setProperty("--ring-alpha", String(clamp(settings.brightness, 0.1, 1)));
  ring!.style.setProperty("--ring-width", `${Math.round(shorter * clamp(settings.thickness, 0.03, 0.25))}px`);
}

let current: RingLightSettings = { brightness: 0.85, warmth: 0.1, thickness: 0.09 };
paint(current);

void listen<RingLightSettings>("ring-light:update", (event) => {
  current = event.payload;
  paint(current);
});
window.addEventListener("resize", () => paint(current));

// The window may be created after settings were last pushed, so ask once.
invoke<StudioState>("get_studio_state")
  .then((state) => {
    current = state.ringLight;
    paint(current);
  })
  .catch(() => undefined);
