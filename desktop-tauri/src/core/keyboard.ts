/** Keyboard shortcut matching and dispatch. Pure logic is exported for testing. */

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** Splits "ctrl+shift+q" into modifiers and key. A trailing "+" is the plus key itself. */
export function parseCombo(combo: string): { key: string; modifiers: Set<string> } {
  const lower = combo.toLowerCase();
  if (lower === "+") return { key: "+", modifiers: new Set() };
  if (lower.endsWith("++")) {
    return { key: "+", modifiers: new Set(lower.slice(0, -2).split("+")) };
  }
  const parts = lower.split("+");
  const key = parts.pop() ?? "";
  return { key, modifiers: new Set(parts) };
}

/**
 * Matches a combo such as "ctrl+q", "f", "?" or "escape".
 *
 * Letters must match their shift state exactly; punctuation ignores Shift,
 * because producing "?" or "+" already requires it on most layouts.
 */
export function matchesCombo(event: KeyLike, combo: string): boolean {
  const { key, modifiers: wants } = parseCombo(combo);

  if (event.ctrlKey !== wants.has("ctrl")) return false;
  if (event.altKey !== wants.has("alt")) return false;
  if (event.metaKey !== wants.has("meta")) return false;

  const pressed = event.key.toLowerCase();
  if (pressed !== key) return false;

  const isLetter = /^[a-z]$/.test(key);
  return isLetter ? event.shiftKey === wants.has("shift") : true;
}

/** True when the user is typing into a field, where single-key shortcuts must not fire. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (typeof HTMLElement === "undefined" || !(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

export interface Shortcut {
  /** One or more combos that trigger the action. */
  keys: string[];
  group: string;
  description: string;
  /** Hidden from the help overlay (for example number keys, listed as one row). */
  hidden?: boolean;
  /** Fires even while a text field has focus (Escape, Ctrl+Q). */
  global?: boolean;
  /** The shortcut is ignored (and the key left alone) while this returns false. */
  when?: () => boolean;
  handler: () => void;
}

export class Keymap {
  private shortcuts: Shortcut[] = [];

  add(...shortcuts: Shortcut[]): void {
    this.shortcuts.push(...shortcuts);
  }

  list(): readonly Shortcut[] {
    return this.shortcuts;
  }

  /** Returns true if a shortcut handled the event. */
  handle(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "target" | "defaultPrevented" | "isComposing" | "preventDefault">): boolean {
    if (event.defaultPrevented || event.isComposing) return false;
    const typing = isTypingTarget(event.target);
    for (const shortcut of this.shortcuts) {
      if (typing && !shortcut.global) continue;
      if (shortcut.when && !shortcut.when()) continue;
      if (shortcut.keys.some((combo) => matchesCombo(event, combo))) {
        event.preventDefault();
        shortcut.handler();
        return true;
      }
    }
    return false;
  }
}

/** Human-friendly label for a combo, e.g. "ctrl+q" -> ["Ctrl", "Q"]. */
export function comboLabels(combo: string): string[] {
  const names: Record<string, string> = {
    ctrl: "Ctrl",
    alt: "Alt",
    shift: "Shift",
    meta: "Super",
    escape: "Esc",
    tab: "Tab",
    " ": "Space",
  };
  const { key, modifiers } = parseCombo(combo);
  return [...modifiers, key].map((part) => names[part] ?? (part.length === 1 ? part.toUpperCase() : part));
}
