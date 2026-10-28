import { describe, expect, it, vi } from "vitest";
import { createStore } from "./store";

describe("createStore", () => {
  it("merges patches and notifies with the previous state", () => {
    const store = createStore({ a: 1, b: "x" });
    const listener = vi.fn();
    store.subscribe(listener);

    store.set({ a: 2 });

    expect(store.get()).toEqual({ a: 2, b: "x" });
    expect(listener).toHaveBeenCalledWith({ a: 2, b: "x" }, { a: 1, b: "x" });
  });

  it("stays quiet when a patch changes nothing", () => {
    const store = createStore({ a: 1 });
    const listener = vi.fn();
    store.subscribe(listener);

    store.set({ a: 1 });

    expect(listener).not.toHaveBeenCalled();
  });

  it("stops notifying after unsubscribe", () => {
    const store = createStore({ a: 1 });
    const listener = vi.fn();
    const unsubscribe = store.subscribe(listener);
    unsubscribe();

    store.set({ a: 2 });

    expect(listener).not.toHaveBeenCalled();
  });

  it("lets a listener unsubscribe itself mid-notification", () => {
    const store = createStore({ a: 1 });
    const second = vi.fn();
    const unsubscribeFirst = store.subscribe(() => unsubscribeFirst());
    store.subscribe(second);

    store.set({ a: 2 });

    expect(second).toHaveBeenCalledTimes(1);
  });
});
