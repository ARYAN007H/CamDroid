/** A tiny typed DOM builder. Text is always inserted as text, never as HTML. */

export type Child = Node | string | null | undefined | false;

type Attributes = Record<string, string | number | boolean | undefined>;

type Listeners = {
  [E in keyof HTMLElementEventMap]?: (event: HTMLElementEventMap[E]) => void;
};

export interface Options {
  class?: string;
  attrs?: Attributes;
  on?: Listeners;
}

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  options: Options = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (options.class) element.className = options.class;
  for (const [name, value] of Object.entries(options.attrs ?? {})) {
    if (value === undefined || value === false) continue;
    element.setAttribute(name, value === true ? "" : String(value));
  }
  for (const [name, listener] of Object.entries(options.on ?? {})) {
    element.addEventListener(name, listener as EventListener);
  }
  append(element, children);
  return element;
}

export function append(parent: ParentNode, children: Child[]): void {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    parent.append(typeof child === "string" ? document.createTextNode(child) : child);
  }
}

/** Replaces everything inside `parent`. */
export function replaceChildren(parent: Element, ...children: Child[]): void {
  parent.replaceChildren();
  append(parent, children);
}

export function toggleClass(element: Element, name: string, on: boolean): void {
  element.classList.toggle(name, on);
}
