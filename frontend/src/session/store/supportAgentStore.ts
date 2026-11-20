import { create } from "zustand";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";
import { refusalSentence } from "../../shared/authorization/refusal";
import { toast } from "../../shared/ui/kit";
import { askSupportAgent, closeSupportAgent, fetchSupportAgent, openSupportAgent } from "../api/support";

/**
 * The caller's Agent de support, as every surface reads it: whether it lives
 * (the daemon says, so a reload finds it again), the « Open support agent »
 * window and its question, and whether its enlarged terminal is up. Opening
 * or asking shows the terminal; minimizing hides it and the agent goes on.
 */
interface SupportAgentState {
  status: SupportStatusResponse | null;
  /** The window, and the question it opens with. */
  dialog: { question: string } | null;
  /** The enlarged terminal of the live agent is up. */
  expanded: boolean;
  /** A gesture is on its way. */
  pending: boolean;
  refresh: () => Promise<void>;
  showDialog: (question?: string) => void;
  hideDialog: () => void;
  expand: () => void;
  collapse: () => void;
  /** Open on `choice` (`replace`: close the one open first); `true` when opened. */
  open: (choice: AgentChoice, question: string, replace: boolean) => Promise<boolean>;
  /** Ask the live agent; `true` when asked. */
  ask: (question: string) => Promise<boolean>;
  end: () => Promise<void>;
}

/** What a failed gesture says: the refusal's sentence, else the daemon's message. */
function failure(error: unknown): void {
  toast(refusalSentence(error) ?? (error instanceof Error ? error.message : String(error)), "error");
}

export const useSupportAgent = create<SupportAgentState>((set, get) => {
  const settle = async (gesture: () => Promise<SupportStatusResponse>): Promise<boolean> => {
    // One gesture at a time: a second Enter while one is on its way does nothing.
    if (get().pending) return false;
    set({ pending: true });
    try {
      const status = await gesture();
      set({ status, pending: false });
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
    refresh: async () => {
      try {
        const status = await fetchSupportAgent();
        set((s) => ({ status, expanded: s.expanded && status.alive }));
      } catch {
        // A daemon that cannot be read keeps what was known.
      }
    },
    showDialog: (question = "") => set({ dialog: { question } }),
    hideDialog: () => set({ dialog: null }),
    expand: () => set((s) => ({ expanded: !!s.status?.alive })),
    collapse: () => set({ expanded: false }),
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
    end: async () => {
      if (await settle(closeSupportAgent)) set({ expanded: false });
    },
  };
});
