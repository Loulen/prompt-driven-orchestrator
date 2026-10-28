import { Bot, FileDiff, X } from "lucide-react";
import type { DraftNotice } from "../../shared/generated/DraftNotice";
import type { Actor } from "../../shared/generated/Actor";
import { assistantOwner, fieldsLabel, myActor } from "../model/draftNotice";
import { dismissDraftNotice } from "../store/draftSessions";
import { useEditStore } from "../store/editStore";
import type { DraftNoticeEntry, OpenPipeline } from "../store/editState";

function Fields({ fields }: { fields: string[] }) {
  if (fields.length === 0) return null;
  return (
    <>
      : <span className="text-text">{fieldsLabel(fields)}</span>
    </>
  );
}

function NoticeText({ notice, me }: { notice: DraftNotice; me: Actor | null }) {
  if (notice.kind === "external_edit") {
    return (
      <span className="min-w-0">
        <span className="font-mono text-text">{notice.file}</span> was changed outside PDO. Kept as{" "}
        <span className="font-mono text-text">
          v{notice.version.number} · {notice.version.title}
        </span>{" "}
        and merged into the draft
        <Fields fields={notice.fields} />.
      </span>
    );
  }
  return (
    <span className="min-w-0">
      {assistantOwner(notice, me)} library assistant changed the draft
      <Fields fields={notice.fields} />.
    </span>
  );
}

/**
 * Under the canvas toolbar (ADR-0088), in the shape of the lint banner: what
 * changed the shared draft from outside the canvas. A file written outside PDO
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
    <div className="mx-3 mt-2 flex flex-col gap-1" data-testid="draft-notices">
      {notices.map(({ id, notice }) => (
        <div
          key={id}
          role="status"
          className="flex items-start gap-2 rounded border border-st-running/40 bg-st-running/14 px-2 py-1.5 text-text-2"
          style={{ fontSize: "10.5px" }}
          data-testid="draft-notice"
          data-kind={notice.kind}
        >
          {notice.kind === "external_edit" ? (
            <FileDiff size={12} className="mt-0.5 shrink-0 text-st-running" />
          ) : (
            <Bot size={12} className="mt-0.5 shrink-0 text-st-running" />
          )}
          <NoticeText notice={notice} me={me} />
          <span className="ml-auto flex shrink-0 items-center gap-1.5">
            {notice.kind === "external_edit" && (
              <button
                type="button"
                onClick={() => onSeeVersion(notice.version.number)}
                className="cursor-pointer whitespace-nowrap text-st-running hover:underline"
                data-testid="draft-notice-see-version"
              >
                See v{notice.version.number}
              </button>
            )}
            <button
              type="button"
              onClick={() => onDismiss(id)}
              aria-label="Dismiss notice"
              className="-mr-0.5 rounded p-0.5 text-text-3 hover:text-text-2 focus-visible:outline focus-visible:outline-1"
              data-testid="draft-notice-dismiss"
            >
              <X size={12} />
            </button>
          </span>
        </div>
      ))}
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
