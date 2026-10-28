import { h, replaceChildren } from "../../core/dom";
import { comboLabels, type Keymap } from "../../core/keyboard";
import { icon } from "../../ui/icons";
import type { Controller } from "./controller";

/** The keyboard cheat sheet, generated from the registered shortcuts so it can never drift. */
export function createHelpOverlay(controller: Controller, keymap: Keymap): { el: HTMLElement; destroy(): void } {
  const groups = h("div", { class: "help-groups" });
  const el = h(
    "div",
    {
      class: "help",
      attrs: { role: "dialog", "aria-label": "Keyboard shortcuts" },
      on: {
        mousedown: (event) => {
          if (event.target === el) controller.toggleHelp();
        },
      },
    },
    h(
      "div",
      { class: "help-card" },
      h(
        "header",
        {},
        h("h2", {}, "Keyboard shortcuts"),
        h(
          "button",
          { class: "icon-btn compact", attrs: { type: "button", "aria-label": "Close" }, on: { click: () => controller.toggleHelp() } },
          icon("close", 18),
        ),
      ),
      groups,
    ),
  );

  function render(): void {
    const byGroup = new Map<string, HTMLElement[]>();
    for (const shortcut of keymap.list()) {
      if (shortcut.hidden && !shortcut.description) continue;
      const rows = byGroup.get(shortcut.group) ?? [];
      const keys = shortcut.keys[0] ?? "";
      rows.push(
        h(
          "div",
          { class: "help-row" },
          h("span", {}, shortcut.description),
          h("span", { class: "help-keys" }, ...comboLabels(keys).map((label) => h("span", { class: "kbd" }, label))),
        ),
      );
      byGroup.set(shortcut.group, rows);
    }
    replaceChildren(
      groups,
      ...[...byGroup].map(([name, rows]) => h("section", { class: "help-group" }, h("h3", {}, name), ...rows)),
    );
  }

  const unsubscribe = controller.store.subscribe((state, previous) => {
    el.classList.toggle("visible", state.helpOpen);
    if (state.helpOpen && !previous.helpOpen) render();
  });
  return { el, destroy: unsubscribe };
}
