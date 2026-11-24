/**
 * The rows of the palette's « Support » group: the caller's Agent de support.
 * Without text: open it (the window), or show the live one and open another.
 * With text: ask it, which opens the window on the text; with an agent open,
 * ask that one at once, without the window, or ask another.
 */

import { useEffect, useMemo } from "react";
import { CircleHelp } from "lucide-react";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";
import { agentTag } from "../../session/model/supportAgent";
import { useSupportAgent } from "../../session/store/supportAgentStore";
import type { BuildInput, Lead, PaletteItem } from "./paletteModel";

const HELP: Lead = { kind: "icon", icon: CircleHelp };

export interface SupportGestures {
  showDialog: (question?: string) => void;
  ask: (question: string) => void;
  expand: () => void;
}

/** The Support rows for the query `text` (`null` under `>`: none). */
export function supportItems(text: string | null, status: SupportStatusResponse | null, gestures: SupportGestures): PaletteItem[] {
  if (text === null) return [];
  const item = (id: string, label: string, run: () => void, extra: Partial<PaletteItem> = {}): PaletteItem => ({
    id: `support:${id}`,
    group: "support",
    label,
    lead: HELP,
    run,
    ...extra,
  });
  const agent = status?.alive ? status.agent : null;
  if (!status?.alive) {
    return text
      ? [item("ask", `Ask the support agent: ${text}`, () => gestures.showDialog(text))]
      : [item("open", "Open support agent…", () => gestures.showDialog())];
  }
  const live: Partial<PaletteItem> = {
    lead: agent ? { kind: "harness", harness: agent.harness } : HELP,
    hint: agent ? `${agentTag(agent)} · open` : "open",
  };
  return text
    ? [
        item("ask-open", `Ask the open agent: ${text}`, () => gestures.ask(text), live),
        item("ask-another", `Ask another agent: ${text}`, () => gestures.showDialog(text)),
      ]
    : [
        item("show", "Open support agent", gestures.expand, live),
        item("open-another", "Open another support agent…", () => gestures.showDialog()),
      ];
}

/** The Support group of the open palette, read fresh as it opens. */
export function useSupportGroup(text: string | null): BuildInput["support"] {
  const status = useSupportAgent((s) => s.status);
  const refresh = useSupportAgent((s) => s.refresh);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  return useMemo(() => {
    const { showDialog, ask, expand } = useSupportAgent.getState();
    return { icon: CircleHelp, items: supportItems(text, status, { showDialog, ask: (q) => void ask(q), expand }) };
  }, [text, status]);
}
