import { create } from "zustand";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { SupportChange } from "../../shared/generated/SupportChange";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";
import { refusalSentence } from "../../shared/authorization/refusal";
import { toast } from "../../shared/ui/kit";
import {
  askSupportAgent,
  closeSupportAgent,
  fetchSupportAgent,
  keepSupportAgent,
  openSupportAgent,
} from "../api/support";
import { answerSeenKey, hasNewAnswer, readSeenAnswer, writeSeenAnswer } from "../model/supportAgent";

/** « Still there? », asked: when the agent closes on this tab's clock, and
 *  after how long without input it was asked. */
export interface StillThere {
  closesAt: number;
  idleTimeoutSecs: number;
}

/**
 * The caller's Agent de support, as every surface reads it: whether it lives
 * (the daemon says, so a reload finds it again), the « Open support agent »
 * window and its question, whether its enlarged terminal is up, « Still
 * there? » while the daemon asks it, and the last answer seen before the
 * terminal was minimized. Opening or asking shows the terminal; minimizing
 * hides it and the agent goes on.
 */
interface SupportAgentState {
  status: SupportStatusResponse | null;
  /** The window, and the question it opens with. */
  dialog: { question: string } | null;
  /** The enlarged terminal of the live agent is up. */
  expanded: boolean;
  /** A gesture is on its way. */
  pending: boolean;
  /** « Still there? », while the daemon asks it. */
  stillThere: StillThere | null;
  /** The agent's last output when its terminal was minimized; `null` when
   *  this agent was never minimized here. */
  seenAnswer: string | null;
  /** When its terminal was last minimized (this tab's clock). */
  collapsedAt: number;
  refresh: () => Promise<void>;
  showDialog: (question?: string) => void;
  hideDialog: () => void;
  expand: () => void;
  collapse: () => void;
  /** Open on `choice` (`replace`: close the one open first); `true` when opened. */
  open: (choice: AgentChoice, question: string, replace: boolean) => Promise<boolean>;
  /** Ask the live agent; `true` when asked. */
  ask: (question: string) => Promise<boolean>;
  /** « Keep »: the agent stays, its idle clock starts over. */
  keep: () => Promise<void>;
  end: () => Promise<void>;
  /** What the daemon said changed of the agent `session` (`/ws`). */
  changed: (session: string, change: SupportChange) => void;
}

/** An answer written this soon after minimizing is the end of what was on
 *  screen, not a new one: the daemon looks at the agents every 5 s, so what it
 *  tells just after a minimize may have been written before. The price: an
 *  answer finished within this delay lights no dot. */
export const SEEN_GRACE_MS = 6_000;

/** What a failed gesture says: the refusal's sentence, else the daemon's message. */
function failure(error: unknown): void {
  toast(refusalSentence(error) ?? (error instanceof Error ? error.message : String(error)), "error");
}

/** « Still there? » as the daemon reports it, on this tab's clock. */
function stillThereOf(status: SupportStatusResponse): StillThere | null {
  const asked = status.alive ? status.still_there : null;
  return asked
    ? { closesAt: Date.now() + asked.closes_in_secs * 1000, idleTimeoutSecs: asked.idle_timeout_secs }
    : null;
}

export const useSupportAgent = create<SupportAgentState>((set, get) => {
  const settle = async (gesture: () => Promise<SupportStatusResponse>): Promise<boolean> => {
    // One gesture at a time: a second Enter while one is on its way does nothing.
    if (get().pending) return false;
    set({ pending: true });
    try {
      const status = await gesture();
      set({ status, pending: false, stillThere: stillThereOf(status) });
      return true;
    } catch (error) {
      set({ pending: false });
      failure(error);
      return false;
    }
  };
  return {
    status: null,
    dialog: null,
    expanded: false,
    pending: false,
    stillThere: null,
    seenAnswer: null,
    collapsedAt: 0,
    refresh: async () => {
      try {
        const status = await fetchSupportAgent();
        set((s) => ({
          status,
          expanded: s.expanded && status.alive,
          stillThere: stillThereOf(status),
          // A reload finds what this browser last saw of this very agent.
          seenAnswer: readSeenAnswer(answerSeenKey(status)),
        }));
      } catch {
        // A daemon that cannot be read keeps what was known.
      }
    },
    showDialog: (question = "") => set({ dialog: { question } }),
    hideDialog: () => set({ dialog: null }),
    expand: () => set((s) => ({ expanded: !!s.status?.alive })),
    collapse: () => {
      const status = get().status;
      // What it wrote so far is read: only a later answer lights the dot.
      const seen = status?.last_output_at ?? "";
      writeSeenAnswer(status ? answerSeenKey(status) : null, seen);
      set({ expanded: false, seenAnswer: seen, collapsedAt: Date.now() });
    },
    open: async (choice, question, replace) => {
      const opened = await settle(() => openSupportAgent(choice, question, replace));
      if (opened) set({ dialog: null, expanded: true });
      return opened;
    },
    ask: async (question) => {
      const asked = await settle(() => askSupportAgent(question));
      if (asked) set({ dialog: null, expanded: true });
      else void get().refresh();
      return asked;
    },
    keep: async () => {
      if (!(await settle(keepSupportAgent))) void get().refresh();
    },
    end: async () => {
      if (await settle(closeSupportAgent)) set({ expanded: false });
    },
    changed: (session, change) => {
      const status = get().status;
      if (!status) {
        void get().refresh();
        return;
      }
      if (status.session !== session) return;
      switch (change.change) {
        case "opened":
          void get().refresh();
          return;
        case "idle":
          set({
            stillThere: {
              closesAt: Date.now() + change.closes_in_secs * 1000,
              idleTimeoutSecs: change.idle_timeout_secs,
            },
          });
          return;
        case "active":
          set({ stillThere: null });
          return;
        case "answered": {
          const s = get();
          const justMinimized = !s.expanded && Date.now() - s.collapsedAt < SEEN_GRACE_MS;
          if (justMinimized) {
            writeSeenAnswer(answerSeenKey(status), change.last_output_at);
            set({ seenAnswer: change.last_output_at });
          }
          set({ status: { ...status, last_output_at: change.last_output_at } });
          return;
        }
        case "closed":
          set({ stillThere: null, expanded: false });
          if (change.idle) toast("The support agent closed: nobody answered “Still there?”", "info");
          void get().refresh();
          return;
      }
    },
  };
});

/** Whether the agent answered since its terminal was minimized here. */
export function useSupportAgentUnread(): boolean {
  return useSupportAgent((s) => !s.expanded && hasNewAnswer(s.status, s.seenAnswer));
}
