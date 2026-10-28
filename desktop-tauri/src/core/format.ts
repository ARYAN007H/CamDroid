export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** "1080p" for 1920x1080, "4K" for 3840x2160. */
export function formatResolution(width: number, height: number): string {
  if (height >= 2160 || width >= 3840) return "4K";
  return `${height}p`;
}

export function formatCodec(codec: string): string {
  switch (codec.toLowerCase()) {
    case "h264":
      return "H.264";
    case "h265":
    case "hevc":
      return "H.265";
    case "mjpeg":
      return "MJPEG";
    default:
      return codec.toUpperCase();
  }
}

export function formatPercent(fraction: number): string {
  return `${Math.round(fraction * 100)}%`;
}

/** "+12", "-8", "0": a signed whole number, for bipolar sliders. */
export function formatSigned(value: number): string {
  const rounded = Math.round(value);
  return rounded > 0 ? `+${rounded}` : `${rounded}`;
}

export function formatCount(value: number): string {
  return new Intl.NumberFormat("en-US").format(value);
}

/** Tauri rejects with the Rust error as a plain string; normalise anything thrown. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}

/** Calls `fn` at most once per animation frame, always with the latest arguments. */
export function rafThrottle<Args extends unknown[]>(fn: (...args: Args) => void): (...args: Args) => void {
  let frame = 0;
  let latest: Args | null = null;
  return (...args: Args) => {
    latest = args;
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (latest) fn(...latest);
    });
  };
}
