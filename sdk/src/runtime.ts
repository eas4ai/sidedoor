// Turns JSX descriptions into plain nodes for the host, and keeps the state
// behind hooks. Hooks belong to a component instance, identified by where it
// sits in the tree (its path) and its `key` or `id`, as in React.

import { Fragment } from "./jsx-runtime";
import type { Child, Component, Element, Node } from "./types";

interface Instance {
  hooks: unknown[];
  cleanups: Map<number, () => void>;
}

interface EffectHook {
  deps?: unknown[];
}

const instances = new Map<string, Instance>();
let current: Instance | null = null;
let cursor = 0;
let seen = new Set<string>();
let pendingEffects: Array<() => void> = [];
/** Event handlers from the latest render, by a key that is stable across renders. */
let handlers = new Map<string, (value: unknown) => void>();
let onInvalidate: () => void = () => {};
let scheduled = false;

/** Called whenever state changes; the host bridge re-renders in response. */
export function setInvalidateHandler(handler: () => void) {
  onInvalidate = handler;
}

/** Schedules one re-render for everything that changed in this tick. */
export function invalidate() {
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(() => {
    scheduled = false;
    onInvalidate();
  });
}

function instance(): Instance {
  if (!current) {
    throw new Error("Hooks can only be called while a component renders.");
  }
  return current;
}

export function useState<T>(
  initial: T | (() => T),
): [T, (next: T | ((previous: T) => T)) => void] {
  const owner = instance();
  const index = cursor++;
  if (!(index in owner.hooks)) {
    owner.hooks[index] =
      typeof initial === "function" ? (initial as () => T)() : initial;
  }
  const set = (next: T | ((previous: T) => T)) => {
    const previous = owner.hooks[index] as T;
    const value =
      typeof next === "function" ? (next as (previous: T) => T)(previous) : next;
    if (!Object.is(value, previous)) {
      owner.hooks[index] = value;
      invalidate();
    }
  };
  return [owner.hooks[index] as T, set];
}

export function useRef<T>(initial: T): { current: T } {
  const owner = instance();
  const index = cursor++;
  if (!(index in owner.hooks)) owner.hooks[index] = { current: initial };
  return owner.hooks[index] as { current: T };
}

function changed(previous: unknown[] | undefined, next: unknown[] | undefined) {
  if (!previous || !next) return true;
  return (
    previous.length !== next.length ||
    previous.some((value, index) => !Object.is(value, next[index]))
  );
}

export function useMemo<T>(compute: () => T, deps: unknown[]): T {
  const owner = instance();
  const index = cursor++;
  const slot = owner.hooks[index] as { deps: unknown[]; value: T } | undefined;
  if (!slot || changed(slot.deps, deps)) {
    owner.hooks[index] = { deps, value: compute() };
  }
  return (owner.hooks[index] as { value: T }).value;
}

/** Runs `effect` after the render reaches the host, again when `deps` change. */
export function useEffect(effect: () => void | (() => void), deps?: unknown[]) {
  const owner = instance();
  const index = cursor++;
  const slot = owner.hooks[index] as EffectHook | undefined;
  if (slot && deps && !changed(slot.deps, deps)) return;
  owner.hooks[index] = { deps };
  pendingEffects.push(() => {
    owner.cleanups.get(index)?.();
    owner.cleanups.delete(index);
    const cleanup = effect();
    if (typeof cleanup === "function") owner.cleanups.set(index, cleanup);
  });
}

/** Calls `tick` every `ms` milliseconds; `null` pauses it. */
export function useInterval(tick: () => void, ms: number | null) {
  const latest = useRef(tick);
  latest.current = tick;
  useEffect(() => {
    if (ms === null) return;
    const timer = setInterval(() => latest.current(), ms);
    return () => clearInterval(timer);
  }, [ms]);
}

/**
 * State shared by the tile, the card and any component that reads it, kept
 * at module level so it outlives any one surface.
 */
export function createStore<T>(initial: T) {
  let value = initial;
  return {
    get: () => value,
    set(next: T | ((previous: T) => T)) {
      const updated =
        typeof next === "function" ? (next as (previous: T) => T)(value) : next;
      if (!Object.is(updated, value)) {
        value = updated;
        invalidate();
      }
    },
    /** Reads the value during render; the widget re-renders when it changes. */
    use: () => value,
  };
}

function isElement(child: unknown): child is Element {
  return typeof child === "object" && child !== null && "type" in child && "props" in child;
}

function nameOf(type: string | Component<any>) {
  return typeof type === "string" ? type : type.name || "Component";
}

/** Prop values as they travel: functions become handler references. */
function encodeProps(props: Record<string, unknown>, path: string) {
  const encoded: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(props)) {
    if (name === "children" || value === undefined) continue;
    if (typeof value === "function") {
      const key = `${path}#${name}`;
      handlers.set(key, value as (value: unknown) => void);
      encoded[name] = { $h: key };
    } else {
      encoded[name] = value;
    }
  }
  return encoded;
}

function renderChild(child: Child, path: string): Node[] {
  if (child === null || child === undefined || typeof child === "boolean") return [];
  if (typeof child === "string" || typeof child === "number") return [String(child)];
  if (Array.isArray(child)) {
    // A keyed item is found by its key wherever it moves; others by position.
    return child.flatMap((item, index) => {
      const keyed = isElement(item) && (item.key ?? item.props.id) !== undefined;
      return renderChild(item, keyed ? path : `${path}.${index}`);
    });
  }
  if (!isElement(child)) return [];

  const identity = child.key ?? (child.props.id as string | number | undefined);
  const here = identity === undefined ? `${path}:${nameOf(child.type)}` : `${path}:${nameOf(child.type)}=${identity}`;

  if (typeof child.type === "function") {
    seen.add(here);
    let owner = instances.get(here);
    if (!owner) {
      owner = { hooks: [], cleanups: new Map() };
      instances.set(here, owner);
    }
    const [outer, outerCursor] = [current, cursor];
    current = owner;
    cursor = 0;
    let output: Child;
    try {
      output = child.type(child.props);
    } finally {
      current = outer;
      cursor = outerCursor;
    }
    return renderChild(output, `${here}/`);
  }

  if (child.type === Fragment) {
    return renderChild(child.props.children, `${here}/`);
  }

  return [
    {
      t: child.type,
      p: encodeProps(child.props, here),
      c: renderChild(child.props.children, `${here}/`),
    },
  ];
}

/**
 * Renders each surface to nodes, unmounting components that disappeared.
 * Effects run afterwards, through `runEffects`.
 */
export function renderSurfaces(surfaces: Record<string, Component>): Record<string, Node[]> {
  seen = new Set();
  handlers = new Map();
  const output: Record<string, Node[]> = {};
  for (const [name, surface] of Object.entries(surfaces)) {
    output[name] = renderChild({ type: surface, props: {} }, name);
  }
  for (const [path, owner] of instances) {
    if (!seen.has(path)) {
      for (const cleanup of owner.cleanups.values()) cleanup();
      instances.delete(path);
    }
  }
  return output;
}

export function runEffects() {
  const effects = pendingEffects;
  pendingEffects = [];
  for (const effect of effects) effect();
}

/** Delivers an event from the host; returns whether a handler took it. */
export function dispatch(key: string, value: unknown): boolean {
  const handler = handlers.get(key);
  if (!handler) return false;
  handler(value);
  return true;
}

/** Forgets every instance; for tests. */
export function reset() {
  for (const owner of instances.values()) {
    for (const cleanup of owner.cleanups.values()) cleanup();
  }
  instances.clear();
  handlers = new Map();
  pendingEffects = [];
}
