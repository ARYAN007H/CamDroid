import { clamp } from "./format";

type Rgb = readonly [number, number, number];

const COOL: Rgb = [214, 230, 255];
const NEUTRAL: Rgb = [255, 246, 232];
const WARM: Rgb = [255, 170, 80];

const mix = (from: Rgb, to: Rgb, amount: number): Rgb => [
  Math.round(from[0] + (to[0] - from[0]) * amount),
  Math.round(from[1] + (to[1] - from[1]) * amount),
  Math.round(from[2] + (to[2] - from[2]) * amount),
];

/** Light colour for a warmth in -1 (cool white) .. +1 (warm amber). */
export function ringLightColor(warmth: number): string {
  const w = clamp(warmth, -1, 1);
  const [r, g, b] = w < 0 ? mix(NEUTRAL, COOL, -w) : mix(NEUTRAL, WARM, w);
  return `rgb(${r}, ${g}, ${b})`;
}
