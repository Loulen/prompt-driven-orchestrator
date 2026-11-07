/**
 * Coordination of the PDO tabs of one browser for the waiting alert (#1012,
 * ADR-0082 §4). Every tab opens its own `/ws` and sees every transition: left
 * alone, N tabs would ring N times. The tabs talk over a local channel
 * (`BroadcastChannel`, else `localStorage`'s `storage` event) to:
 *
 * - **aggregate the visibility**: « no PDO tab in view » is judged over every
 *   tab — each one announces whether it is visible, on change and on a
 *   heartbeat, and says goodbye when it goes away;
 * - **elect one emitter per transition** (first who claims): a tab that is
 *   about to alert posts a claim for the transition, waits a short window, and
 *   emits only if its claim is the earliest one seen (the smallest tab id
 *   breaks a tie) — every tab ranks the claims the same way. A tab the browser
 *   would not yet let play a sound (autoplay policy: the user never interacted
 *   with it) claims a beat later, so a tab that can ring is preferred. No tab
 *   leads for good: a closed tab simply stops claiming and the next transition
 *   goes to another one; a tab closing inside its own claim window hands that
 *   transition to the next claimer;
 * - **share the sound window**: one chime per 10 s across every tab.
 *
 * The core (`createTabCoordinator`) takes its transport, clock and timers as
 * arguments, so it is unit-tested without a browser; `initTabCoordinator`
 * wires the browser's.
 */

export type TabMessage =
  | { type: "hello"; tabId: string; visible: boolean; soundAt: number | null }
  | { type: "state"; tabId: string; visible: boolean; soundAt: number | null }
  | { type: "bye"; tabId: string }
  | { type: "claim"; tabId: string; key: string; ts: number }
  | { type: "sound"; tabId: string; at: number };

export interface TabTransport {
  post(message: TabMessage): void;
  subscribe(handler: (message: TabMessage) => void): () => void;
  close(): void;
}

export interface TabCoordinatorOptions {
  transport: TabTransport;
  tabId: string;
  /** Is THIS tab in view? */
  isVisible: () => boolean;
  /** May THIS tab start a sound now (autoplay policy)? */
  canPlaySound: () => boolean;
  now?: () => number;
  setTimeout?: (fn: () => void, ms: number) => unknown;
  /** How long a claim waits for a competing one before emitting. */
  claimWindowMs?: number;
  /** How much later a tab that may not play sound yet posts its claim. */
  mutedDelayMs?: number;
  /** A peer's « I am visible » is trusted this long without news (a crashed tab). */
  visibleTtlMs?: number;
  /** How long a claim is remembered (a late tab must still see it). */
  claimTtlMs?: number;
}

export interface TabCoordinator {
  readonly tabId: string;
  /** Is a PDO tab of this browser in view — this one or any other? */
  anyTabVisible(): boolean;
  /** Claim the emission of one transition. Resolves `true` if this tab emits it. */
  claim(key: string): Promise<boolean>;
  /** When the last chime played in any tab (epoch ms), `null` if never. */
  lastSoundAt(): number | null;
  /** This tab plays a chime now: tell the others. */
  noteSound(at: number): void;
  /** Tell the others where this tab stands (visibility change, heartbeat). */
  announce(): void;
  /** This tab goes away: say goodbye and stop listening. */
  dispose(): void;
}

export const CLAIM_WINDOW_MS = 250;
export const MUTED_DELAY_MS = 100;
export const VISIBLE_TTL_MS = 15_000;
export const HEARTBEAT_MS = 5_000;
const CLAIM_TTL_MS = 60_000;

export interface Claim {
  tabId: string;
  ts: number;
}

/** Same order in every tab: the earliest, then the smallest tab id. */
export function rankClaims(a: Claim, b: Claim): number {
  if (a.ts !== b.ts) return a.ts - b.ts;
  return a.tabId < b.tabId ? -1 : a.tabId > b.tabId ? 1 : 0;
}

export function createTabCoordinator(options: TabCoordinatorOptions): TabCoordinator {
  const {
    transport,
    tabId,
    isVisible,
    canPlaySound,
    now = () => Date.now(),
    setTimeout: schedule = (fn, ms) => globalThis.setTimeout(fn, ms),
    claimWindowMs = CLAIM_WINDOW_MS,
    mutedDelayMs = MUTED_DELAY_MS,
    visibleTtlMs = VISIBLE_TTL_MS,
    claimTtlMs = CLAIM_TTL_MS,
  } = options;

  const peers = new Map<string, { visible: boolean; seenAt: number }>();
  const claims = new Map<string, { claims: Claim[]; seenAt: number }>();
  let soundAt: number | null = null;
  let disposed = false;

  const prune = () => {
    const t = now();
    for (const [key, entry] of claims) {
      if (t - entry.seenAt > claimTtlMs) claims.delete(key);
    }
  };

  const recordClaim = (key: string, claim: Claim) => {
    const entry = claims.get(key) ?? { claims: [], seenAt: now() };
    if (!entry.claims.some((c) => c.tabId === claim.tabId)) entry.claims.push(claim);
    entry.seenAt = now();
    claims.set(key, entry);
  };

  const mergeSound = (at: number | null | undefined) => {
    if (typeof at !== "number") return;
    soundAt = soundAt === null ? at : Math.max(soundAt, at);
  };
  // Every greeting carries the last chime: a tab opened (or reloaded) inside a
  // sound window knows it at once.
  const state = (): TabMessage => ({ type: "state", tabId, visible: isVisible(), soundAt });

  const unsubscribe = transport.subscribe((message) => {
    if (disposed || message.tabId === tabId) return;
    switch (message.type) {
      case "hello":
        peers.set(message.tabId, { visible: message.visible, seenAt: now() });
        mergeSound(message.soundAt);
        transport.post(state());
        break;
      case "state":
        peers.set(message.tabId, { visible: message.visible, seenAt: now() });
        mergeSound(message.soundAt);
        break;
      case "bye":
        peers.delete(message.tabId);
        // Its claims still inside their window fall (it never emitted them): a
        // concurrent claimer takes the transition over. A claim whose window
        // has closed was emitted — it stays, so nobody emits it twice.
        for (const entry of claims.values()) {
          const t = now();
          entry.claims = entry.claims.filter((c) => c.tabId !== message.tabId || t >= c.ts + claimWindowMs);
        }
        break;
      case "claim":
        prune();
        recordClaim(message.key, { tabId: message.tabId, ts: message.ts });
        break;
      case "sound":
        mergeSound(message.at);
        break;
    }
  });

  transport.post({ type: "hello", tabId, visible: isVisible(), soundAt });

  return {
    tabId,
    anyTabVisible() {
      if (isVisible()) return true;
      const t = now();
      for (const peer of peers.values()) {
        if (peer.visible && t - peer.seenAt <= visibleTtlMs) return true;
      }
      return false;
    },
    claim(key) {
      if (disposed) return Promise.resolve(false);
      const post = (resolve: (won: boolean) => void) => {
        if (disposed) return resolve(false);
        prune();
        const mine: Claim = { tabId, ts: now() };
        recordClaim(key, mine);
        transport.post({ type: "claim", key, ...mine });
        schedule(() => {
          if (disposed) return resolve(false);
          const all = [...(claims.get(key)?.claims ?? [mine])].sort(rankClaims);
          resolve(all[0]?.tabId === tabId);
        }, claimWindowMs);
      };
      return new Promise((resolve) => {
        if (canPlaySound()) post(resolve);
        else schedule(() => post(resolve), mutedDelayMs);
      });
    },
    lastSoundAt() {
      return soundAt;
    },
    noteSound(at) {
      mergeSound(at);
      if (!disposed) transport.post({ type: "sound", tabId, at });
    },
    announce() {
      if (!disposed) transport.post(state());
    },
    dispose() {
      if (disposed) return;
      transport.post({ type: "bye", tabId });
      disposed = true;
      unsubscribe();
      transport.close();
    },
  };
}

// ---------------------------------------------------------------------------
// Transports

export const TAB_CHANNEL_NAME = "pdo.waitingAlerts";
/** The `localStorage` key the fallback transport writes its messages through. */
export const TAB_BUS_STORAGE_KEY = "pdo.ui.waitingAlerts.bus";

function isTabMessage(value: unknown): value is TabMessage {
  return !!value && typeof value === "object" && typeof (value as TabMessage).type === "string" && typeof (value as TabMessage).tabId === "string";
}

export function broadcastChannelTransport(name = TAB_CHANNEL_NAME): TabTransport | null {
  if (typeof BroadcastChannel !== "function") return null;
  let channel: BroadcastChannel;
  try {
    channel = new BroadcastChannel(name);
  } catch {
    return null;
  }
  return {
    post: (message) => {
      try {
        channel.postMessage(message);
      } catch {
        // closed channel: the tab is going away
      }
    },
    subscribe: (handler) => {
      const listener = (e: MessageEvent) => {
        if (isTabMessage(e.data)) handler(e.data);
      };
      channel.addEventListener("message", listener);
      return () => channel.removeEventListener("message", listener);
    },
    close: () => channel.close(),
  };
}

/**
 * The fallback: each message is written to one `localStorage` key; the other
 * tabs read it off the `storage` event (never fired in the writing tab).
 */
export function localStorageTransport(key = TAB_BUS_STORAGE_KEY): TabTransport | null {
  if (typeof window === "undefined" || !window.addEventListener) return null;
  try {
    if (!window.localStorage) return null;
  } catch {
    return null;
  }
  return {
    post: (message) => {
      try {
        localStorage.setItem(key, JSON.stringify({ message, nonce: Math.random() }));
      } catch {
        // full or disabled store: this tab stays on its own
      }
    },
    subscribe: (handler) => {
      const listener = (e: StorageEvent) => {
        if (e.key !== key || !e.newValue) return;
        try {
          const { message } = JSON.parse(e.newValue) as { message: unknown };
          if (isTabMessage(message)) handler(message);
        } catch {
          // not ours
        }
      };
      window.addEventListener("storage", listener);
      return () => window.removeEventListener("storage", listener);
    },
    close: () => {},
  };
}

/** A transport that reaches nobody: one tab alone. */
export function loneTransport(): TabTransport {
  return { post: () => {}, subscribe: () => () => {}, close: () => {} };
}

/**
 * In-memory bus for tests: every transport it hands out reaches the others
 * (not itself), synchronously like a same-process channel.
 */
export function createMemoryBus() {
  const handlers = new Set<{ owner: object; fn: (m: TabMessage) => void }>();
  const posted: TabMessage[] = [];
  return {
    posted,
    transport(): TabTransport {
      const owner = {};
      return {
        post: (message) => {
          posted.push(message);
          for (const h of [...handlers]) if (h.owner !== owner) h.fn(structuredClone(message));
        },
        subscribe: (fn) => {
          const entry = { owner, fn };
          handlers.add(entry);
          return () => handlers.delete(entry);
        },
        close: () => {
          for (const h of [...handlers]) if (h.owner === owner) handlers.delete(h);
        },
      };
    },
  };
}

// ---------------------------------------------------------------------------
// The browser's coordinator, one per page

let current: TabCoordinator | null = null;
let unwire: (() => void) | null = null;

function newTabId(): string {
  try {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  } catch {
    // non-secure context: no randomUUID
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export interface InitTabCoordinatorOptions {
  canPlaySound: () => boolean;
  claimWindowMs?: number;
}

/**
 * Start this page's coordinator: announce it, follow its visibility, beat its
 * heart, and say goodbye when the page goes away. Idempotent per page.
 */
export function initTabCoordinator(options: InitTabCoordinatorOptions): TabCoordinator {
  disposeTabCoordinator();
  const transport = broadcastChannelTransport() ?? localStorageTransport() ?? loneTransport();
  const isVisible = () => typeof document === "undefined" || document.visibilityState === "visible";
  const coordinator = createTabCoordinator({
    transport,
    tabId: newTabId(),
    isVisible,
    canPlaySound: options.canPlaySound,
    claimWindowMs: options.claimWindowMs,
  });
  current = coordinator;

  if (typeof window !== "undefined" && window.addEventListener) {
    const onVisibility = () => coordinator.announce();
    const onPageHide = () => coordinator.dispose();
    const onPageShow = (e: PageTransitionEvent) => {
      // Back from the bfcache: the old coordinator said goodbye, start anew.
      if (e.persisted && current === coordinator) initTabCoordinator(options);
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", onPageHide);
    window.addEventListener("pageshow", onPageShow);
    const heartbeat = setInterval(() => coordinator.announce(), HEARTBEAT_MS);
    unwire = () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("pagehide", onPageHide);
      window.removeEventListener("pageshow", onPageShow);
      clearInterval(heartbeat);
    };
  }
  return coordinator;
}

/** This page's coordinator, started on first use. */
export function getTabCoordinator(options: InitTabCoordinatorOptions): TabCoordinator {
  return current ?? initTabCoordinator(options);
}

/** Stop this page's coordinator (test seam, and `initTabCoordinator`'s reset). */
export function disposeTabCoordinator(): void {
  unwire?.();
  unwire = null;
  current?.dispose();
  current = null;
}
