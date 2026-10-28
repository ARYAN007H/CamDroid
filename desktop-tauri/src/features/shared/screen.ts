/** A full-window view. The app mounts one at a time and destroys it when leaving. */
export interface Screen {
  el: HTMLElement;
  /** Moves keyboard focus to the screen's primary control. */
  focus?(): void;
  /** Removes listeners and timers. Called once, when the screen is replaced. */
  destroy(): void;
}
