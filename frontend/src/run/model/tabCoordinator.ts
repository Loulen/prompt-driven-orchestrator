/**
 * The PDO tabs of one browser, coordinated for the Alerte d'attente (ADR
 * draft-alertes-d-attente §5). Every tab opens its own `/ws` and sees every
 * transition: alone, N tabs would ring N times. The tabs talk over a local
 * channel (`BroadcastChannel`, else the `storage` event) to:
 *
 * - **aggregate the visibility**: « no PDO tab in view » is judged over every
 *   tab; each one announces whether it is visible, on a change and on a
 *   heartbeat, and says goodbye when it goes away;
 * - **elect one emitter per transition**: a tab about to alert posts a claim,
 *   waits a short window, and emits only if its claim is the earliest seen (the
 *   smallest tab id breaks a tie); every tab ranks the claims the same way. A
 *   tab the browser would not yet let ring (autoplay policy) claims a beat
 *   later, so a tab that can ring is preferred. No tab leads for good: a closed
 *   tab stops claiming, and one closing inside its own window hands the
 *   transition to the next claimer;
 * - **share the sound window**: one chime per 10 s across every tab.
 *
 * Pure core: the transport, the clock and the timers are arguments, so it is
 * unit-tested without a browser (`run/store/tabCoordinatorBrowser.ts` wires
 * the browser's).
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
