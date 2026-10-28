import { History } from "lucide-react";
import type { RunState } from "../../shared/generated/RunState";
import { lastVersionOf, versionLabel } from "../model/versions";
import { useEditStore } from "../store/editStore";
import "./inspect.css";

/**
 * Under the head of Pipeline info (ADR-0088): the version a Run launched,
 * the last one of a template, the one a read-only tab shows; on a Run, the
 * « vN is out » pill after it.
 */
export function InfoVersionLine({ run, pipelineId }: { run: RunState | null; pipelineId: string | null }) {
  const pipelines = useEditStore((s) => s.pipelines);
  const shown = useEditStore((s) => s.openTabs.find((t) => t.id === s.activeTabId)?.version);
  const launched = run?.pipeline_version ?? null;
  const line = run
    ? launched
      ? `launched ${versionLabel(launched)}`
      : null
    : shown
      ? `${versionLabel({ number: shown.number, title: shown.title })} · read only`
      : versionLabel(lastVersionOf(pipelines, pipelineId ?? ""));
  if (line == null) return null;
  return (
    <div className="pi-vline">
      <History size={12} strokeWidth={1.5} aria-hidden className="ei-ic" />
      <span className="d-mono" title={line} data-testid="info-panel-version">
        {line}
      </span>
      <NewerVersionPill run={run} />
    </div>
  );
}

/** « vN is out »: a Run launched an older version than the pipeline's last; it opens the pipeline. */
function NewerVersionPill({ run }: { run: RunState | null }) {
  const pipelines = useEditStore((s) => s.pipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const launched = run?.pipeline_version ?? null;
  if (!run || !launched) return null;
  // A Run created by the pipeline's stem carries no id: its name stands for it.
  const id = run.pipeline_id ?? run.pipeline_name;
  const last = lastVersionOf(pipelines, id);
  if (last.number <= launched.number) return null;
  return (
    <button
      type="button"
      className="d-chip pi-newer"
      onClick={() => void openPipeline(id)}
      data-testid="newer-version"
      title={`Launched v${launched.number}; the pipeline is at v${last.number} now`}
    >
      <History size={11} strokeWidth={1.5} aria-hidden />v{last.number} is out
    </button>
  );
}
