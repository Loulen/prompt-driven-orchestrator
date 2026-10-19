import { useActiveDraft, useOthers } from "../store/useDraftPresence";
import { ParticipantStack } from "./ParticipantAvatar";

/**
 * The shared draft of the active tab, in the tab bar (ADR-0088): who else has
 * it open, and its changes since the file. A tab that only reads says so.
 */
export default function DraftPresence() {
  const draft = useActiveDraft();
  const participants = useOthers();
  if (!draft) return null;
  const changes = draft.changes === 1 ? "1 change" : `${draft.changes} changes`;
  return (
    <span className="flex items-center gap-2" data-testid="draft-presence">
      <ParticipantStack participants={participants} />
      {draft.synced && !draft.canWrite && (
        <span
          className="rounded border border-line-strong px-1 font-mono text-fg-3"
          style={{ fontSize: "10px" }}
          data-testid="draft-read-only"
          title="You may read this pipeline, not change it"
        >
          read only
        </span>
      )}
      {draft.status === "offline" && (
        <span className="font-mono text-st-await" style={{ fontSize: "10px" }} data-testid="draft-offline">
          offline
        </span>
      )}
      {draft.changes > 0 && (
        <span className="whitespace-nowrap font-mono text-fg-4" style={{ fontSize: "10px" }} data-testid="draft-changes">
          draft · {changes}
        </span>
      )}
    </span>
  );
}
