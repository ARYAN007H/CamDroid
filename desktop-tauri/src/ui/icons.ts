/**
 * The icon set: 24x24, 1.8px round strokes, drawn to match each other.
 * Markup is static and trusted, so it is safe to assign as HTML.
 */
const PATHS = {
  search: '<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4-4"/>',
  globe:
    '<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18"/>',
  usb:
    '<circle cx="12" cy="19.5" r="1.8"/><path d="M12 17.7V4"/><path d="m12 4-2.2 2.7M12 4l2.2 2.7"/><path d="m12 13 4.5-2.4V8.6"/><rect x="15" y="5.4" width="3.2" height="3.2" rx=".6"/><path d="m12 15 -4-2v-1.4"/><circle cx="8" cy="10.2" r="1.3"/>',
  sparkles:
    '<path d="M11 3.5 12.9 8.6 18 10.5l-5.1 1.9L11 17.5l-1.9-5.1L4 10.5l5.1-1.9z"/><path d="m18.5 15.5.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z"/>',
  flip: '<path d="M12 3v18"/><path d="M8.5 7 3.5 17h5z"/><path d="m15.5 7 5 10h-5z"/>',
  sun:
    '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>',
  video:
    '<rect x="2.5" y="6" width="13" height="12" rx="2.5"/><path d="m15.5 10.5 6-3.5v10l-6-3.5z"/>',
  switchCamera:
    '<path d="M20 11a8 8 0 0 0-14.5-4.5L4 8"/><path d="M4 4v4h4"/><path d="M4 13a8 8 0 0 0 14.5 4.5L20 16"/><path d="M20 20v-4h-4"/>',
  sliders:
    '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
  close: '<path d="m6 6 12 12M18 6 6 18"/>',
  power: '<path d="M12 3v8"/><path d="M6.4 6.8a8 8 0 1 0 11.2 0"/>',
  chevronRight: '<path d="m9.5 6 6 6-6 6"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7"/>',
  alert: '<path d="M12 4 2.5 20h19z"/><path d="M12 10v4.2"/><circle cx="12" cy="17.2" r=".5"/>',
  battery: '<rect x="2" y="7.5" width="17" height="9" rx="2.5"/><path d="M21.5 10.5v3"/>',
  flash: '<path d="M13 2.5 4.5 13.5H11l-1 8 8.5-11H12z"/>',
  help:
    '<circle cx="12" cy="12" r="9"/><path d="M9.6 9.4a2.5 2.5 0 1 1 3.5 2.3c-.7.4-1.1.9-1.1 1.7"/><circle cx="12" cy="16.8" r=".5"/>',
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/>',
  phone: '<rect x="7" y="2.5" width="10" height="19" rx="2.6"/><path d="M10.8 18.5h2.4"/>',
  unplugged:
    '<path d="m4 4 16 16"/><path d="M8.5 8.5A6 6 0 0 0 6 13.5V16h8"/><path d="M16 10.5V8M12.5 6V4M10 7V4"/>',
} as const;

export type IconName = keyof typeof PATHS;

export function icon(name: IconName, size = 20): HTMLSpanElement {
  const wrapper = document.createElement("span");
  wrapper.className = "icon";
  wrapper.setAttribute("aria-hidden", "true");
  wrapper.innerHTML =
    `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" ` +
    `stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${PATHS[name]}</svg>`;
  return wrapper;
}
