import type { FilterPresetId } from "../../api/types";

/**
 * Flat swatches that suggest each look. Static on purpose: rendering a live
 * thumbnail per preset would cost more than the effects themselves.
 */
export const PRESET_SWATCHES: Record<FilterPresetId, string> = {
  natural: "linear-gradient(160deg, #5f7ea3 0%, #d9bf9e 60%, #4a3b33 100%)",
  warm_film: "linear-gradient(160deg, #7d7f8f 0%, #f0c48a 55%, #5a3a26 100%)",
  teal_orange: "linear-gradient(160deg, #1f6f78 0%, #e3a06a 60%, #3b2a1f 100%)",
  noir: "linear-gradient(160deg, #262626 0%, #cfcfcf 55%, #101010 100%)",
  vintage: "linear-gradient(160deg, #8a8f7a 0%, #e0c79a 55%, #5b4a38 100%)",
  pixel: "repeating-conic-gradient(#e9c7a1 0% 25%, #5b7da6 0% 50%) 0 0 / 14px 14px",
  vhs:
    "repeating-linear-gradient(0deg, rgba(0,0,0,0.3) 0 2px, transparent 2px 4px), " +
    "linear-gradient(160deg, #6a5fa5, #e8a3b8 55%, #2f3a5a)",
  neon_edge:
    "linear-gradient(135deg, transparent 38%, #00e5ff 39% 42%, transparent 43%), " +
    "linear-gradient(45deg, transparent 56%, #ff2bd6 57% 60%, transparent 61%), #0b0b16",
  cartoon: "linear-gradient(160deg, #3b82f6 0 33%, #f59e0b 33% 66%, #ef4444 66%)",
};
