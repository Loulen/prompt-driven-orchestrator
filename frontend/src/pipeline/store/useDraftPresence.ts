/**
 * What the editor shows of the other participants of the active tab's shared
 * draft (ADR-0088).
 */

import { useMemo } from "react";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import type { SelectionKind } from "../../shared/generated/SelectionKind";
import { others } from "../model/presence";
import { useEditStore } from "./editStore";
import type { DraftTabState } from "./editState";

/** The shared draft of the active tab; `undefined` on a run tab. */
export function useActiveDraft(): DraftTabState | undefined {
  return useEditStore((s) => s.openTabs.find((t) => t.id === s.activeTabId)?.draft);
}

/** The other participants of the active tab's draft. */
export function useOthers(): DraftParticipant[] {
  const draft = useActiveDraft();
  return useMemo(() => (draft ? others(draft.participants, draft.connection) : []), [draft]);
}

/** The other participant that selected `kind` `id`, if one did. */
export function useSelectedByOther(kind: SelectionKind, id: string): DraftParticipant | null {
  const participants = useOthers();
  return useMemo(
    () => participants.find((p) => p.selection?.kind === kind && p.selection.id === id) ?? null,
    [participants, kind, id],
  );
}

/** The other participants typing in the prompt of `nodeId`. */
export function useTypingInPrompt(nodeId: string): DraftParticipant[] {
  const participants = useOthers();
  return useMemo(
    () => participants.filter((p) => p.typing?.node_id === nodeId && p.typing.field === "prompt"),
    [participants, nodeId],
  );
}
