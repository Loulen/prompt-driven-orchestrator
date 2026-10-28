import { CircleDashed, History, Info, RotateCcw, X } from "lucide-react";
import type { Actor } from "../../shared/generated/Actor";
import type { RunState } from "../../shared/generated/RunState";
import { actorLabel } from "../../shared/lib/actor";
import { useActorDisplay } from "../../shared/actors/store";
import { Avatar, Button, Chip, IconButton } from "../../shared/ui/kit";
import { restoreIntoDraft } from "../store/restoreIntoDraft";
import { changesLabel, lastVersionOf, savedOn, versionLabel } from "../model/versions";
import type { OpenPipeline, VersionTabState } from "../store/editState";
import { useEditStore } from "../store/editStore";
import { requestInfoTab } from "../store/infoRequest";
import "./inspect.css";

/** Who saved a version: their avatar and name, or « file » when nobody in the app did. */
export function SavedBy({ actor }: { actor: Actor | null }) {
  const display = useActorDisplay(actor);
  if (!actor) return <span className="ei-dim">file</span>;
  const name = display?.name ?? actorLabel(actor);
  return (
    <span className="pi-by" title={name}>
      <Avatar name={name} src={display?.avatar_url} size={16} />
      <span className="ei-trunc">{name}</span>
    </span>
  );
}

/**
 * A version shown read only (ADR-0088), one line under the tab bar like the
 * lint: which version, who saved it and when, « Restore into draft ».
 */
export default function VersionBanner({
  version,
  infoOpen = false,
  onToggleInfo,
}: {
  version: VersionTabState;
  /** Pipeline info, where the history is: the toolbar's `(i)`. */
  infoOpen?: boolean;
  onToggleInfo?: () => void;
}) {
  const restoreVersion = useEditStore((s) => s.restoreVersion);
  const closeTab = useEditStore((s) => s.closeTab);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const title = version.title.trim();
  return (
    <div className="pi-vban" data-kind="viewing" role="status" data-testid="version-banner">
      {onToggleInfo && (
        <IconButton
          icon={Info}
          label="Pipeline info"
          size="sm"
          pressed={infoOpen}
          onClick={onToggleInfo}
          data-testid="version-banner-info"
        />
      )}
      <History size={13} strokeWidth={1.5} aria-hidden className="pi-vban__ic" />
      <span className="d-mono pi-vban__v">v{version.number}</span>
      {title !== "" && title !== `v${version.number}` && (
        <span className="ei-trunc" title={title}>
          {title}
        </span>
      )}
      <Chip>read only</Chip>
      <SavedBy actor={version.author} />
      <span className="d-mono ei-dim" title={new Date(version.savedAt).toLocaleString("en-US")}>
        {savedOn(version.savedAt)}
      </span>
      <span className="pi-vban__end">
        <Button
          size="sm"
          tone="primary"
          icon={RotateCcw}
          onClick={() => void restoreIntoDraft(restoreVersion, version.pipelineId, version.number)}
          data-testid="version-banner-restore"
        >
          Restore into draft
        </Button>
        {activeTabId && (
          <IconButton
            icon={X}
            label={`Close v${version.number}`}
            size="sm"
            onClick={() => closeTab(activeTabId)}
            data-testid="version-banner-close"
          />
        )}
      </span>
    </div>
  );
}

/**
 * The line under the tab bar (ADR-0088): a version shown read only, a Run
 * launched from an older version than the pipeline's last, or a draft that
 * differs from its last version. Nothing otherwise.
 */
export function VersionLine({
  tab,
  run,
  infoOpen,
  onToggleInfo,
}: {
  tab: OpenPipeline;
  run: RunState | null;
  infoOpen?: boolean;
  onToggleInfo?: () => void;
}) {
  const pipelines = useEditStore((s) => s.pipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  if (tab.version) return <VersionBanner version={tab.version} infoOpen={infoOpen} onToggleInfo={onToggleInfo} />;
  if (tab.runId != null) {
    const launched = run?.pipeline_version ?? null;
    if (!run || !launched) return null;
    const pipelineId = run.pipeline_id ?? run.pipeline_name;
    const last = lastVersionOf(pipelines, pipelineId);
    if (last.number <= launched.number) return null;
    return (
      <div className="pi-vban" data-kind="launched" role="status" data-testid="version-line-launched">
        <History size={13} strokeWidth={1.5} aria-hidden className="ei-ic" />
        <span className="ei-dim">Launched</span>
        <span className="d-mono ei-trunc" title={versionLabel(launched)}>
          {versionLabel(launched)}
        </span>
        <span className="d-chip pi-newer">v{last.number} is out</span>
        <span className="pi-vban__end">
          <Button size="sm" tone="ghost" onClick={() => void openPipeline(pipelineId)}>
            Open pipeline
          </Button>
        </span>
      </div>
    );
  }
  const changes = tab.draft?.changes ?? 0;
  if (changes === 0) return null;
  const last = lastVersionOf(pipelines, tab.id);
  return (
    <div className="pi-vban" data-kind="draft" role="status" data-testid="version-line-draft">
      <CircleDashed size={13} strokeWidth={1.5} aria-hidden className="ei-ic" />
      <span>Draft</span>
      <span className="d-mono ei-dim ei-trunc" data-testid="draft-changes">
        {changesLabel(changes)} since v{last.number}
      </span>
      <span className="pi-vban__end">
        <Button size="sm" tone="ghost" icon={History} onClick={() => requestInfoTab("versions")}>
          Versions
        </Button>
      </span>
    </div>
  );
}
