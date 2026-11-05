import { describe, expect, it } from "vitest";
import { computeFeedTransform } from "./feed-geometry";

describe("computeFeedTransform", () => {
  it("passes the full frame through for source aspect", () => {
    const t = computeFeedTransform(1920, 1080, "source", 0);
    expect(t).toEqual({ sx: 0, sy: 0, sw: 1920, sh: 1080, width: 1920, height: 1080, rotation: 0 });
  });

  it("crops a centered square out of a 16:9 frame", () => {
    const t = computeFeedTransform(1920, 1080, "1:1", 0);
    expect(t.sw).toBe(1080);
    expect(t.sh).toBe(1080);
    expect(t.sx).toBe(420);
    expect(t.sy).toBe(0);
    expect(t.width).toBe(1080);
    expect(t.height).toBe(1080);
  });

  it("crops a portrait 9:16 window out of a 16:9 frame", () => {
    const t = computeFeedTransform(1920, 1080, "9:16", 0);
    expect(t.sw).toBe(608); // 1080 * 9/16, rounded
    expect(t.sh).toBe(1080);
    expect(t.sx).toBe(656);
    expect(t.width).toBe(608);
    expect(t.height).toBe(1080);
  });

  it("widens to 4:3 by trimming the sides of a 16:9 frame", () => {
    const t = computeFeedTransform(1920, 1080, "4:3", 0);
    expect(t.sw).toBe(1440); // 1080 * 4/3
    expect(t.sh).toBe(1080);
    expect(t.sx).toBe(240);
  });

  it("swaps the canvas dimensions for quarter-turn rotations", () => {
    const t = computeFeedTransform(1920, 1080, "source", 90);
    expect(t.width).toBe(1080);
    expect(t.height).toBe(1920);
    expect(t.rotation).toBe(90);
    expect(t.sw).toBe(1920);
    expect(t.sh).toBe(1080);
  });

  it("keeps a square crop square under 270° rotation", () => {
    const t = computeFeedTransform(1920, 1080, "1:1", 270);
    expect(t.width).toBe(1080);
    expect(t.height).toBe(1080);
    expect(t.rotation).toBe(270);
  });
});
