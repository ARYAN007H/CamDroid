import { describe, expect, it } from "vitest";
import { ringLightColor } from "./ring-light-color";

describe("ringLightColor", () => {
  it("hits the three anchor colours", () => {
    expect(ringLightColor(-1)).toBe("rgb(214, 230, 255)");
    expect(ringLightColor(0)).toBe("rgb(255, 246, 232)");
    expect(ringLightColor(1)).toBe("rgb(255, 170, 80)");
  });

  it("gets warmer (less blue) as warmth increases", () => {
    const blue = (css: string) => Number(/rgb\(\d+, \d+, (\d+)\)/.exec(css)?.[1]);
    expect(blue(ringLightColor(0.5))).toBeLessThan(blue(ringLightColor(0)));
    expect(blue(ringLightColor(-0.5))).toBeGreaterThan(blue(ringLightColor(0)));
  });

  it("clamps out-of-range input", () => {
    expect(ringLightColor(9)).toBe(ringLightColor(1));
    expect(ringLightColor(-9)).toBe(ringLightColor(-1));
  });
});
