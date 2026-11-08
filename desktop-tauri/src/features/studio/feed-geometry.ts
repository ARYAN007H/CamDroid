import type { FeedAspect, FeedRotation } from "../../api/types";

export interface FeedTransform {
  /** Centered source-crop rectangle, in source pixels. */
  sx: number;
  sy: number;
  sw: number;
  sh: number;
  /** Output bitmap size: the crop, swapped for quarter-turn rotations. */
  width: number;
  height: number;
  /** Clockwise rotation to apply while drawing, in degrees. */
  rotation: FeedRotation;
}

const ASPECT_RATIOS: Record<Exclude<FeedAspect, "source">, number> = {
  "16:9": 16 / 9,
  "4:3": 4 / 3,
  "1:1": 1,
  "9:16": 9 / 16,
};

/**
 * Geometry for rendering the feed on the studio stage: the largest centered
 * crop of the source that matches the chosen aspect, rotated clockwise.
 * The canvas is sized to the rotated crop so CSS `object-fit: contain`
 * letterboxes every combination without extra styling.
 */
export function computeFeedTransform(
  sourceWidth: number,
  sourceHeight: number,
  aspect: FeedAspect,
  rotation: FeedRotation,
): FeedTransform {
  let sw = sourceWidth;
  let sh = sourceHeight;
  if (aspect !== "source" && sourceWidth > 0 && sourceHeight > 0) {
    const target = ASPECT_RATIOS[aspect];
    const source = sourceWidth / sourceHeight;
    if (target > source) {
      sh = Math.round(sourceWidth / target);
    } else if (target < source) {
      sw = Math.round(sourceHeight * target);
    }
  }
  const quarter = rotation === 90 || rotation === 270;
  return {
    sx: (sourceWidth - sw) / 2,
    sy: (sourceHeight - sh) / 2,
    sw,
    sh,
    width: quarter ? sh : sw,
    height: quarter ? sw : sh,
    rotation,
  };
}
