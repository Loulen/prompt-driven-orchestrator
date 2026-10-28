import type { RunState } from "../../shared/generated/RunState";
import WaitingForStartedNodesBanner from "../../run/components/WaitingForStartedNodesBanner";
import type { OpenPipeline } from "../store/editState";
import { DraftNotices } from "./DraftNoticeBanner";
import LintBanner, { type LintBannerItem } from "./LintBanner";

/**
 * The lines under the tab bar, one each, full width, above the canvas: a Run
 * that waits at End for its started nodes, what changed the shared draft
 * from outside the canvas, the lint. Nothing when none applies.
 */
export default function CanvasBands({
  tab,
  run,
  lint,
  onDismissLint,
}: {
  tab: OpenPipeline;
  run: RunState | null;
  lint: LintBannerItem[];
  onDismissLint: (id: string) => void;
}) {
  // #225: lint is an edit-mode affordance — suppressed on run tabs (and on a
  // version shown read only, ADR-0088).
  const template = tab.runId == null && tab.version == null;
  const shown =
    (run?.waiting_for_started_nodes ?? 0) > 0 || (template && (lint.length > 0 || (tab.draft?.notices.length ?? 0) > 0));
  if (!shown) return null;
  return (
    <div className="shrink-0">
      <WaitingForStartedNodesBanner run={run} />
      {/* ADR-0088: what changed the shared draft from outside the canvas (a
          file written outside PDO, the library assistant), above the lint. */}
      {template && (
        <>
          <DraftNotices tab={tab} />
          {lint.length > 0 && <LintBanner items={lint} onDismiss={onDismissLint} />}
        </>
      )}
    </div>
  );
}
