import { Archive } from "lucide-react";
import { filePath } from "../../run/model/runRefs";
import ReviewFileCard from "./ReviewFileCard";
import ReviewSendBar from "./ReviewSendBar";
import type { DiffFile } from "../../shared/generated/DiffFile";
import type { RefPair, ViewMode } from "../../run/model/runRefs";
import type { ReviewCommentsApi } from "../model/commentsApi";
import type { Anchor, ReviewDraft } from "../model/drafts";
import type { ReviewEntry } from "../model/entries";
import type { Load } from "../store/useReviewDiff";

interface Props {
  runId: string;
  pair: RefPair;
  isArchived: boolean;
  sameRef: boolean;
  current: Load;
  reload: () => void;
  files: DiffFile[];
  effectiveView: ViewMode;
  collapsedSet: Set<string>;
  toggleFile: (path: string) => void;
  registerEl: (path: string, el: HTMLDivElement | null) => void;
  entriesByPath: Map<string, ReviewEntry[]>;
  commentsApi: ReviewCommentsApi;
  pairDrafts: ReviewDraft[];
  sentCount: number;
  sendingKeys: ReadonlySet<string>;
  managerRunning: boolean;
  sendDisabledReason: string | null;
  jumpTo: (anchor: Anchor) => void;
  send: (keys: string[]) => Promise<void>;
}

/**
 * The body of the Review page (#749): an archived Run, a pair with nothing to
 * compare, the loading and error states, no changes, or every file card of the
 * diff followed by the sticky send bar of the pair's drafts (#750).
 */
export default function ReviewDiffBody({
  runId,
  pair,
  isArchived,
  sameRef,
  current,
  reload,
  files,
  effectiveView,
  collapsedSet,
  toggleFile,
  registerEl,
  entriesByPath,
  commentsApi,
  pairDrafts,
  sentCount,
  sendingKeys,
  managerRunning,
  sendDisabledReason,
  jumpTo,
  send,
}: Props) {
  return (
    <>
      {isArchived ? (
        <EmptyState
          testId="review-archived"
          icon={<Archive size={22} className="text-text-3" />}
          title="Diff not preserved for archived runs"
          hint="The run branch was deleted at cleanup."
        />
      ) : sameRef ? (
        <EmptyState
          testId="review-empty"
          icon={<span className="text-text-3" style={{ fontSize: "24px" }}>≡</span>}
          title="Nothing to compare"
          hint="Source and destination are the same ref. Pick another destination."
        />
      ) : current.kind === "none" ? (
        <div className="px-4 py-10 text-center text-text-3" style={{ fontSize: "11px" }} data-testid="review-loading">
          Loading diff…
        </div>
      ) : current.kind === "error" ? (
        <div className="flex flex-col items-center gap-2 px-4 py-14 text-center" data-testid="review-error">
          <div className="text-text-3" style={{ fontSize: "12px" }}>
            {current.status === 404 ? "Run branch not found" : "Could not load the diff"}
          </div>
          <div className="text-text-3" style={{ fontSize: "11px" }}>
            {current.message}
          </div>
          <button
            type="button"
            onClick={reload}
            className="mt-1 cursor-pointer rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-2 hover:text-text"
            style={{ fontSize: "10.5px" }}
          >
            Retry
          </button>
        </div>
      ) : files.length === 0 ? (
        <EmptyState
          testId="review-empty"
          icon={<span className="text-text-3" style={{ fontSize: "24px" }}>≡</span>}
          title="No changes"
          hint="The two refs have identical trees."
        />
      ) : (
        <>
          {files.length > 40 && (
            <div className="mx-3 mt-2.5 rounded border border-line bg-surface-2 px-2.5 py-1 text-text-3" style={{ fontSize: "10.5px" }}>
              Large diff — files collapsed
            </div>
          )}
          {files.map((f) => {
            const p = filePath(f);
            return (
              <ReviewFileCard
                key={`${pair.from}|${pair.to}|${p}`}
                runId={runId}
                file={f}
                pair={pair}
                view={effectiveView}
                collapsed={collapsedSet.has(p)}
                onToggle={() => toggleFile(p)}
                registerEl={registerEl}
                entries={entriesByPath.get(p)}
                comments={commentsApi}
              />
            );
          })}
          <ReviewSendBar
            drafts={pairDrafts}
            sentCount={sentCount}
            sending={sendingKeys.size > 0}
            managerRunning={managerRunning}
            sendDisabledReason={sendDisabledReason}
            onJump={(d) => jumpTo(d)}
            onSendAll={() => void send(pairDrafts.map((d) => d.key))}
          />
          <div className="h-[calc(40vh/var(--app-zoom,1))]" />
        </>
      )}
    </>
  );
}

function EmptyState({
  testId,
  icon,
  title,
  hint,
}: {
  testId: string;
  icon: React.ReactNode;
  title: string;
  hint: string;
}) {
  return (
    <div className="flex flex-col items-center gap-1 px-4 pt-20 text-center" data-testid={testId}>
      <div className="mb-1">{icon}</div>
      <div className="text-text-3" style={{ fontSize: "12px" }}>
        {title}
      </div>
      <div className="text-text-3" style={{ fontSize: "11px" }}>
        {hint}
      </div>
    </div>
  );
}
