import { Eye } from "lucide-react";
import { Chip } from "../../shared/ui/kit";
import { useActiveDraft, useOthers } from "../store/useDraftPresence";
import { ParticipantStack } from "./ParticipantAvatar";

/**
 * Who else has the active tab's shared draft open (ADR-0088), at the top
 * right of the canvas: their avatars, and whether this tab only reads or is
 * offline. The draft's changes since the last version are on the line under
 * the tab bar.
 */
export default function DraftPresence() {
  const draft = useActiveDraft();
  const participants = useOthers();
  if (!draft) return null;
  const readOnly = draft.synced && !draft.canWrite;
  const offline = draft.status === "offline";
  if (!readOnly && !offline && participants.length === 0) return null;
  return (
    <span className="pi-pres" data-testid="draft-presence">
      {readOnly && (
        <span data-testid="draft-read-only">
          <Chip icon={Eye} mono title="You may read this pipeline, not change it">
            read only
          </Chip>
        </span>
      )}
      {offline && (
        <span data-testid="draft-offline">
          <Chip tone="awaiting" mono title="Draft offline">
            offline
          </Chip>
        </span>
      )}
      <ParticipantStack participants={participants} />
    </span>
  );
}
