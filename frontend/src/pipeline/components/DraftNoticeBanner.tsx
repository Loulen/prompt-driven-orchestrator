import { Bot, FileDiff, X } from "lucide-react";
import type { DraftNotice } from "../../shared/generated/DraftNotice";
import type { Actor } from "../../shared/generated/Actor";
import { assistantOwner, fieldsLabel, myActor } from "../model/draftNotice";
import { dismissDraftNotice } from "../store/draftSessions";
import { versionLabel } from "../model/versions";
import { Button, IconButton } from "../../shared/ui/kit";
import "./inspect.css";
import { useEditStore } from "../store/editStore";
import type { DraftNoticeEntry, OpenPipeline } from "../store/editState";

/** The notice in words, for its line and its tooltip. */
function noticeText(notice: DraftNotice, me: Actor | null): string {
  const fields = fieldsLabel(notice.fields);
  if (notice.kind === "external_edit") {
    const kept = `kept as ${versionLabel(notice.version)}`;
    return [`${notice.file} changed outside`, kept, fields && `merged: ${fields}`].filter(Boolean).join(" · ");
  }
  return [`${assistantOwner(notice, me)} library assistant`, fields].filter(Boolean).join(" · ");
}

/**
 * Under the tab bar (ADR-0088), one line each in the shape of the lint: what
 * changed the shared draft from outside the canvas. A file written outside
 * was kept as a version (« See vN » opens it) and merged in; the library
 * assistant saved into it. No conflict to resolve: × only dismisses.
 */
export default function DraftNoticeBanner({
  notices,
  me,
  onSeeVersion,
  onDismiss,
}: {
  notices: DraftNoticeEntry[];
  /** The Actor of this tab: its own assistant is « Your ». */
  me: Actor | null;
  onSeeVersion: (number: number) => void;
  onDismiss: (id: number) => void;
}) {
  if (notices.length === 0) return null;
  return (
    <div data-testid="draft-notices">
      {notices.map(({ id, notice }) => {
        const text = noticeText(notice, me);
        return (
          <div key={id} role="status" className="pi-notice" data-testid="draft-notice" data-kind={notice.kind}>
            {notice.kind === "external_edit" ? (
              <FileDiff size={13} strokeWidth={1.5} aria-hidden />
            ) : (
              <Bot size={13} strokeWidth={1.5} aria-hidden />
            )}
            <span className="ei-trunc" title={text}>
              {text}
            </span>
            <span className="pi-vban__end">
              {notice.kind === "external_edit" && (
                <Button
                  size="sm"
                  tone="ghost"
                  onClick={() => onSeeVersion(notice.version.number)}
                  data-testid="draft-notice-see-version"
                >
                  See v{notice.version.number}
                </Button>
              )}
              <IconButton icon={X} size="sm" label="Dismiss" onClick={() => onDismiss(id)} data-testid="draft-notice-dismiss" />
            </span>
          </div>
        );
      })}
    </div>
  );
}

/** The notices of `tab`'s shared draft, wired to the store. */
export function DraftNotices({ tab }: { tab: OpenPipeline }) {
  const openPipelineVersion = useEditStore((s) => s.openPipelineVersion);
  if (!tab.draft) return null;
  return (
    <DraftNoticeBanner
      notices={tab.draft.notices}
      me={myActor(tab.draft.participants, tab.draft.connection)}
      onSeeVersion={(number) => void openPipelineVersion(tab.id, number)}
      onDismiss={(id) => dismissDraftNotice(tab.id, id)}
    />
  );
}
