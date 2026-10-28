import { describe, expect, it, vi } from "vitest";
import { comboLabels, Keymap, matchesCombo, parseCombo, type KeyLike } from "./keyboard";

const press = (key: string, modifiers: Partial<KeyLike> = {}): KeyLike => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...modifiers,
});

describe("matchesCombo", () => {
  it("matches letters regardless of Caps Lock, but keeps Shift+letter distinct", () => {
    expect(matchesCombo(press("f"), "f")).toBe(true);
    // Caps Lock reports an upper-case key without Shift; the shortcut must still work.
    expect(matchesCombo(press("F"), "f")).toBe(true);
    expect(matchesCombo(press("F", { shiftKey: true }), "f")).toBe(false);
    expect(matchesCombo(press("F", { shiftKey: true }), "shift+f")).toBe(true);
  });

  it("parses a lone plus key and plus-with-modifiers", () => {
    expect(parseCombo("+")).toEqual({ key: "+", modifiers: new Set() });
    expect(parseCombo("ctrl++")).toEqual({ key: "+", modifiers: new Set(["ctrl"]) });
    expect(parseCombo("ctrl+shift+q")).toEqual({ key: "q", modifiers: new Set(["ctrl", "shift"]) });
  });

  it("requires modifiers to match exactly", () => {
    expect(matchesCombo(press("q", { ctrlKey: true }), "ctrl+q")).toBe(true);
    expect(matchesCombo(press("q"), "ctrl+q")).toBe(false);
    expect(matchesCombo(press("q", { ctrlKey: true, altKey: true }), "ctrl+q")).toBe(false);
    expect(matchesCombo(press("q", { ctrlKey: true }), "q")).toBe(false);
  });

  it("ignores Shift for punctuation that needs it to be typed", () => {
    expect(matchesCombo(press("?", { shiftKey: true }), "?")).toBe(true);
    expect(matchesCombo(press("+", { shiftKey: true }), "+")).toBe(true);
    expect(matchesCombo(press("["), "[")).toBe(true);
  });

  it("matches named keys", () => {
    expect(matchesCombo(press("Escape"), "escape")).toBe(true);
    expect(matchesCombo(press("Tab"), "tab")).toBe(true);
    expect(matchesCombo(press("1"), "1")).toBe(true);
  });
});

describe("comboLabels", () => {
  it("renders readable key caps", () => {
    expect(comboLabels("ctrl+q")).toEqual(["Ctrl", "Q"]);
    expect(comboLabels("escape")).toEqual(["Esc"]);
    expect(comboLabels("?")).toEqual(["?"]);
    expect(comboLabels("[")).toEqual(["["]);
    expect(comboLabels("+")).toEqual(["+"]);
    expect(comboLabels("ctrl+q")).toEqual(["Ctrl", "Q"]);
  });
});

describe("Keymap", () => {
  const event = (key: string, extra: Partial<KeyLike> = {}) => ({
    ...press(key, extra),
    target: null,
    defaultPrevented: false,
    isComposing: false,
    preventDefault: vi.fn(),
  });

  it("runs the matching handler and claims the key", () => {
    const handler = vi.fn();
    const keymap = new Keymap();
    keymap.add({ keys: ["m"], group: "Studio", description: "Mirror", handler });
    const pressed = event("m");

    expect(keymap.handle(pressed)).toBe(true);
    expect(handler).toHaveBeenCalledOnce();
    expect(pressed.preventDefault).toHaveBeenCalled();
  });

  it("skips shortcuts whose guard is false and leaves the key alone", () => {
    const handler = vi.fn();
    const keymap = new Keymap();
    keymap.add({ keys: ["m"], group: "Studio", description: "Mirror", when: () => false, handler });
    const pressed = event("m");

    expect(keymap.handle(pressed)).toBe(false);
    expect(handler).not.toHaveBeenCalled();
    expect(pressed.preventDefault).not.toHaveBeenCalled();
  });

  it("lets a later shortcut handle the key when an earlier guard declines", () => {
    const first = vi.fn();
    const second = vi.fn();
    const keymap = new Keymap();
    keymap.add(
      { keys: ["1"], group: "A", description: "first", when: () => false, handler: first },
      { keys: ["1"], group: "B", description: "second", handler: second },
    );

    keymap.handle(event("1"));

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledOnce();
  });

  it("ignores events that were already handled or are part of text composition", () => {
    const handler = vi.fn();
    const keymap = new Keymap();
    keymap.add({ keys: ["m"], group: "Studio", description: "Mirror", handler });

    expect(keymap.handle({ ...event("m"), defaultPrevented: true })).toBe(false);
    expect(keymap.handle({ ...event("m"), isComposing: true })).toBe(false);
    expect(handler).not.toHaveBeenCalled();
  });
});
