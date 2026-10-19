import type { RunState } from "../../shared/generated/RunState";
import { lastVersionOf, versionLabel } from "../model/versions";
import { useEditStore } from "../store/editStore";

/**
 * Under the name in Pipeline info (ADR-0088): the version a Run launched,
 * the last one of a template, the one a read-only tab shows.
 */
export function InfoVersionLine({ run, pipelineId }: { run: RunState | null; pipelineId: string | null }) {
  const pipelines = useEditStore((s) => s.pipelines);
  const shown = useEditStore((s) => s.openTabs.find((t) => t.id === s.activeTabId)?.version);
  const launched = run?.pipeline_version ?? null;
  const line = run
    ? `run ${run.run_id.slice(-8)}${launched ? ` · ${versionLabel(launched)}` : ""}`
    : shown
      ? `version · ${versionLabel({ number: shown.number, title: shown.title })} · read only`
      : `template · ${versionLabel(lastVersionOf(pipelines, pipelineId ?? ""))}`;
  return (
    <div className="mt-0.5 truncate font-mono text-fg-4" style={{ fontSize: "10px" }} data-testid="info-panel-version">
      {line}
    </div>
  );
}

/** « vN is out »: a Run launched an older version than the pipeline's last. */
export function NewerVersionPill({ run }: { run: RunState | null }) {
  const pipelines = useEditStore((s) => s.pipelines);
  const launched = run?.pipeline_version ?? null;
  if (!run || !launched) return null;
  // A Run created by the pipeline's stem carries no id: its name stands for it.
  const last = lastVersionOf(pipelines, run.pipeline_id ?? run.pipeline_name);
  if (last.number <= launched.number) return null;
  return (
    <span
      className="shrink-0 rounded border border-st-await/40 bg-st-await-bg px-1.5 py-0.5 font-mono text-st-await"
      style={{ fontSize: "9.5px" }}
      data-testid="newer-version"
      title={`This Run launched v${launched.number}; the pipeline is at v${last.number} now`}
    >
      v{last.number} is out
    </span>
  );
}
