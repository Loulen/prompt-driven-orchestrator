/**
 * The browser side of the tab coordination (`run/model/tabCoordinator.ts`):
 * the local channels between the PDO tabs of this browser, and this page's
 * coordinator, started once at boot so every PDO page (the Review page too)
 * tells the others whether it is in view.
 */
import {
  createTabCoordinator,
  HEARTBEAT_MS,
  type TabCoordinator,
  type TabMessage,
  type TabTransport,
} from "../model/tabCoordinator";
import { canPlaySoundNow } from "./soundAdapter";
import { newTabId } from "./useDaemonSocket";

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

// ---------------------------------------------------------------------------
// The browser's coordinator, one per page

let current: TabCoordinator | null = null;
let unwire: (() => void) | null = null;

/**
 * Start this page's coordinator: announce it, follow its visibility, beat its
 * heart, and say goodbye when the page goes away. Idempotent per page.
 */
export function initTabCoordinator(): TabCoordinator {
  disposeTabCoordinator();
  const transport = broadcastChannelTransport() ?? localStorageTransport() ?? loneTransport();
  const isVisible = () => typeof document === "undefined" || document.visibilityState === "visible";
  const coordinator = createTabCoordinator({ transport, tabId: newTabId(), isVisible, canPlaySound: canPlaySoundNow });
  current = coordinator;

  if (typeof window !== "undefined" && window.addEventListener) {
    const onVisibility = () => coordinator.announce();
    const onPageHide = () => coordinator.dispose();
    const onPageShow = (e: PageTransitionEvent) => {
      // Back from the bfcache: the old coordinator said goodbye, start anew.
      if (e.persisted && current === coordinator) initTabCoordinator();
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
export function getTabCoordinator(): TabCoordinator {
  return current ?? initTabCoordinator();
}

/** Stop this page's coordinator (test seam, and `initTabCoordinator`'s reset). */
export function disposeTabCoordinator(): void {
  unwire?.();
  unwire = null;
  current?.dispose();
  current = null;
}
