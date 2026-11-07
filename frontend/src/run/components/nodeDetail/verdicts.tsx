import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { verdictHeadline, verdictTone } from "../../model/nodeStatus";
import type { StatusTone } from "../../../shared/ui/kit/tones";
import NodeBanner from "./NodeBanner";

const MARK_TONE: Record<ReturnType<typeof verdictTone>, StatusTone> = {
  done: "done",
  await: "awaiting",
  failed: "failed",
  stopped: "skipped",
};

/** A headline as the banner's fact and detail: « Refused — reason » splits at the dash. */
function split(headline: string): { title: string; text?: string } {
  const at = headline.indexOf(" — ");
  return at < 0 ? { title: headline } : { title: headline.slice(0, at), text: headline.slice(at + 3) };
}

/**
 * The verdict of an Approve click, rendered at the gesture (#490).
 *
 * `data-*` attributes rather than copy are the assertion surface: a level-5 driver
 * asserts `data-recoverable="true"` for "still your turn" and `"false"` for "the node
 * is failed now", so the journey survives a rewording.
 */
export function MarkCompleteVerdict({ verdict }: { verdict: MarkVerdict }) {
  const refused = verdict.kind === "refused" ? verdict : null;
  const next =
    refused?.recoverable === true
      ? "fix the above, then approve again"
      : refused?.recoverable === false
        ? "resume the run to try again"
        : null;
  const { title, text } = split(verdictHeadline(verdict));
  return (
    <NodeBanner
      tone={MARK_TONE[verdictTone(verdict)]}
      title={title}
      text={text}
      testId="mark-complete-verdict"
      tip={[title, text, next].filter(Boolean).join(" · ")}
      data={{
        "data-verdict": verdict.kind,
        "data-slug": refused?.slug ?? "",
        "data-recoverable": refused ? String(refused.recoverable) : "",
      }}
    >
      {refused && refused.missing.length > 0 && (
        <ul className="ri-banner__list" data-testid="verdict-missing-list">
          {/* The `Missing outputs:` prefix is load-bearing — the gating e2e spec
              matches `/^Missing outputs:/`. */}
          <li>Missing outputs: {refused.missing.join(", ")}</li>
        </ul>
      )}
      {refused && refused.violations.length > 0 && (
        <ul className="ri-banner__list" data-testid="verdict-violation-list">
          {refused.violations.map((v, i) => (
            <li key={i}>
              {v.port}.{v.field}: {v.reason}
            </li>
          ))}
        </ul>
      )}
    </NodeBanner>
  );
}

export function ReleaseCompletionVerdict({
  verdict,
}: {
  verdict: ReleaseVerdict | { kind: "released" };
}) {
  const refused = verdict.kind === "refused" ? verdict : null;
  const tone: StatusTone =
    verdict.kind === "released"
      ? "running"
      : verdict.kind === "refused" || verdict.kind === "error"
        ? "failed"
        : "skipped";
  const { title, text } =
    verdict.kind === "released"
      ? { title: "Auto-approve armed", text: "the agent completes when ready" }
      : verdict.kind === "pending"
        ? { title: "Arming auto-approve…", text: undefined }
        : verdict.kind === "error"
          ? { title: "Could not reach the daemon", text: verdict.message }
          : { title: "Auto-approve refused", text: verdict.message };
  const next =
    verdict.kind === "released"
      ? "Tell the agent in the terminal that you are done; Approve still forces it."
      : verdict.kind === "refused"
        ? "Retry the node, then choose Auto-approve when ready again."
        : null;
  return (
    <NodeBanner
      tone={tone}
      title={title}
      text={text}
      testId="release-verdict"
      tip={[title, text, next].filter(Boolean).join(" · ")}
      data={{
        "data-verdict": verdict.kind,
        "data-slug": refused?.slug ?? "",
        "data-recoverable": refused ? String(refused.recoverable) : "",
      }}
    />
  );
}
