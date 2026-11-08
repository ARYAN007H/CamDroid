export type Listener<T> = (state: Readonly<T>, previous: Readonly<T>) => void;

/** A minimal observable state container; changes notify subscribers synchronously. */
export interface Store<T> {
  get(): Readonly<T>;
  /** Shallow-merges `patch`. Does nothing (and notifies nobody) if nothing changed. */
  set(patch: Partial<T>): void;
  subscribe(listener: Listener<T>): () => void;
}

export function createStore<T extends object>(initial: T): Store<T> {
  let state: Readonly<T> = initial;
  const listeners = new Set<Listener<T>>();

  return {
    get: () => state,
    set(patch) {
      const keys = Object.keys(patch) as (keyof T)[];
      if (keys.every((key) => Object.is(state[key], patch[key]))) return;
      const previous = state;
      state = { ...state, ...patch };
      for (const listener of [...listeners]) listener(state, previous);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
