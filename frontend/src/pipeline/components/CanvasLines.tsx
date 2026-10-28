import type { RunState } from "../../shared/generated/RunState";
import WaitingForStartedNodesBanner from "../../run/components/WaitingForStartedNodesBanner";
import type { OpenPipeline } from "../store/editState";
import { DraftNotices } from "./DraftNoticeBanner";
import LintBanner, { type LintBannerItem } from "./LintBanner";
import { VersionLine } from "./VersionBanner";

/**
 * The lines under the tab bar, full width, above the canvas (ADR-0088): the
 * version shown read only, an older version a Run launched, or the draft's
 * changes; a Run that waits at End for its started nodes; then what changed
 * the shared draft from outside the canvas (a file written outside, the
 * library assistant), then the lint. #225: lint is an edit-mode affordance —
 * suppressed on run tabs.
 */
export default function CanvasLines({
  tab,
  run,
  infoOpen,
  onToggleInfo,
  lint,
  onDismissLint,
}: {
  tab: OpenPipeline;
  run: RunState | null;
  infoOpen?: boolean;
  onToggleInfo?: () => void;
  lint: LintBannerItem[];
  onDismissLint: (id: string) => void;
}) {
  return (
    <div className="shrink-0">
      <VersionLine tab={tab} run={run} infoOpen={infoOpen} onToggleInfo={onToggleInfo} />
      <WaitingForStartedNodesBanner run={run} />
      {tab.runId == null && tab.version == null && (
        <>
          <DraftNotices tab={tab} />
          {lint.length > 0 && <LintBanner items={lint} onDismiss={onDismissLint} />}
        </>
      )}
    </div>
  );
}
