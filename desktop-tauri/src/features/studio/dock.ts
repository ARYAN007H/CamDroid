import { h } from "../../core/dom";
import { icon, type IconName } from "../../ui/icons";
import type { AppState } from "../shared/app-state";
import type { Controller } from "../shared/controller";

interface DockButton {
  label: string;
  shortcut: string;
  icon: IconName;
  action: () => void;
  pressed: (state: Readonly<AppState>) => boolean;
  /** A small amber dot signalling a problem with this feature. */
  warn?: (state: Readonly<AppState>) => boolean;
}

/** The control strip at the bottom: icons only, with a tooltip and shortcut on hover. */
export function createDock(controller: Controller): { el: HTMLElement; destroy(): void } {
  const groups: DockButton[][] = [
    [
      {
        label: "Looks",
        shortcut: "E",
        icon: "sparkles",
        action: () => controller.toggleTray(),
        pressed: (s) => s.trayOpen,
      },
      {
        label: "Mirror",
        shortcut: "M",
        icon: "flip",
        action: () => void controller.toggleMirror(),
        pressed: (s) => s.studio.mirror,
      },
      {
        label: "Ring light",
        shortcut: "R",
        icon: "sun",
        action: () => void controller.toggleRingLight(),
        pressed: (s) => s.studio.ringLightVisible,
      },
    ],
    [
      {
        label: "Virtual camera",
        shortcut: "O",
        icon: "video",
        action: () => void controller.toggleVirtualCamera(),
        pressed: (s) => s.studio.outputMode === "virtual_camera",
        warn: (s) =>
          s.studio.outputMode === "virtual_camera" && s.session?.telemetry.virtualCamera.state === "failed",
      },
      {
        label: "Flip phone camera",
        shortcut: "C",
        icon: "switchCamera",
        action: () => controller.switchCamera(),
        pressed: () => false,
      },
    ],
    [
      {
        label: "Adjust",
        shortcut: "I",
        icon: "sliders",
        action: () => controller.togglePanel(),
        pressed: (s) => s.panelOpen,
      },
    ],
  ];

  const bound = groups.flat().map((spec) => {
    const dot = h("span", { class: "badge-dot" });
    const button = h(
      "button",
      {
        class: "icon-btn",
        attrs: {
          type: "button",
          "aria-label": spec.label,
          "aria-pressed": false,
          "data-tip": `${spec.label}  ${spec.shortcut}`,
        },
        on: { click: spec.action },
      },
      icon(spec.icon, 22),
      dot,
    );
    return { spec, button, dot };
  });

  const children: Node[] = [];
  let index = 0;
  groups.forEach((group, groupIndex) => {
    if (groupIndex > 0) children.push(h("span", { class: "sep" }));
    for (let i = 0; i < group.length; i++) children.push(bound[index++]!.button);
  });
  const el = h("nav", { class: "dock", attrs: { "aria-label": "Studio controls" } }, ...children);

  function sync(state: Readonly<AppState>): void {
    for (const { spec, button, dot } of bound) {
      button.setAttribute("aria-pressed", String(spec.pressed(state)));
      dot.hidden = !(spec.warn?.(state) ?? false);
    }
  }

  const unsubscribe = controller.store.subscribe(sync);
  sync(controller.store.get());
  return { el, destroy: unsubscribe };
}
