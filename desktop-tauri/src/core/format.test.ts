import { describe, expect, it } from "vitest";
import {
  clamp,
  errorMessage,
  formatCodec,
  formatCount,
  formatPercent,
  formatResolution,
  formatSigned,
} from "./format";

describe("formatResolution", () => {
  it("labels common modes the way people say them", () => {
    expect(formatResolution(1920, 1080)).toBe("1080p");
    expect(formatResolution(2560, 1440)).toBe("1440p");
    expect(formatResolution(3840, 2160)).toBe("4K");
  });
});

describe("formatCodec", () => {
  it("uses the familiar spellings", () => {
    expect(formatCodec("h264")).toBe("H.264");
    expect(formatCodec("HEVC")).toBe("H.265");
    expect(formatCodec("mjpeg")).toBe("MJPEG");
    expect(formatCodec("av1")).toBe("AV1");
  });
});

describe("number formatting", () => {
  it("formats percentages and signed offsets", () => {
    expect(formatPercent(0.856)).toBe("86%");
    expect(formatSigned(12.4)).toBe("+12");
    expect(formatSigned(-7.6)).toBe("-8");
    expect(formatSigned(0.2)).toBe("0");
    expect(formatCount(1234567)).toBe("1,234,567");
  });

  it("clamps into a range", () => {
    expect(clamp(5, 0, 3)).toBe(3);
    expect(clamp(-1, 0, 3)).toBe(0);
    expect(clamp(2, 0, 3)).toBe(2);
  });
});

describe("errorMessage", () => {
  it("accepts the plain strings Tauri rejects with", () => {
    expect(errorMessage("The phone did not answer.")).toBe("The phone did not answer.");
  });

  it("accepts Error objects and falls back for anything else", () => {
    expect(errorMessage(new Error("boom"))).toBe("boom");
    expect(errorMessage({ weird: true })).toBe("Something went wrong.");
  });
});
