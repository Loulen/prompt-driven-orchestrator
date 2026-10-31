import { useEffect, useState } from "react";
import { CircleAlert, CircleDashed, Eye, Loader, Play, RotateCcw } from "lucide-react";
import type { PipelineVersionEntry } from "../../shared/generated/PipelineVersionEntry";
import type { Actor } from "../../shared/generated/Actor";
import { actorLabel } from "../../shared/lib/actor";
import { useActorDisplay } from "../../shared/actors/store";
import { Avatar, Button, Chip, EmptyState, IconButton } from "../../shared/ui/kit";
import { fetchPipelineVersions } from "../api/versions";
import { changesLabel, lastVersionOf, shortSavedAgo, versionLabel } from "../model/versions";
import { useEditStore } from "../store/editStore";
import { ParticipantStack } from "./ParticipantAvatar";
import { restoreIntoDraft } from "../store/restoreIntoDraft";
import "./inspect.css";

/** Who saved a version: its avatar, or « file » when nobody in the app did. */
function Author({ actor, external }: { actor: Actor | null; external: boolean }) {
  const display = useActorDisplay(actor);
  if (!actor) {
    return (
      <span className="pi-vrow__by d-mono" title={external ? "Written outside" : "The file the versions started from"}>
        file
      </span>
    );
  }
  const name = display?.name ?? actorLabel(actor);
  return (
    <span className="pi-vrow__by" title={name}>
      <Avatar name={name} actor={actor} src={display?.avatar_url} size={16} />
    </span>
  );
}

const fullDate = (iso: string) =>
  new Date(iso).toLocaleString("en-US", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

/**
 * The « Versions » tab of a pipeline (ADR-0088): its draft first, then every
 * version a Save froze, newest first, one row each (number, title, author,
 * age, Runs). « View » opens a version read only; « Restore » makes it the
 * content of the draft, for every participant, as one change Undo takes
 * back. Neither creates a version.
 */
export default function VersionsTab({ pipelineId }: { pipelineId: string }) {
  const pipelines = useEditStore((s) => s.pipelines);
  const tabDraft = useEditStore((s) => s.openTabs.find((t) => t.id === pipelineId)?.draft);
  const viewing = useEditStore((s) => s.openTabs.find((t) => t.id === s.activeTabId)?.version);
  // A Replace keeps the number and the title: the save's time says it moved.
  const savedAt = useEditStore((s) => s.lastSavedAt[pipelineId]);
  const openPipelineVersion = useEditStore((s) => s.openPipelineVersion);
  const restoreVersion = useEditStore((s) => s.restoreVersion);
  const last = lastVersionOf(pipelines, pipelineId);
  const listed = pipelines.find((p) => p.id === pipelineId);
  const changes = tabDraft?.changes ?? listed?.draft?.changes ?? 0;
  const participants = listed?.draft?.participants ?? tabDraft?.participants ?? [];
  const [versions, setVersions] = useState<PipelineVersionEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  // The ages read « now », « 5m »…: they follow the clock, not the last read.
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 30_000);
    return () => clearInterval(id);
  }, []);

  // Read again when the last version moves (a Save, here or elsewhere).
  useEffect(() => {
    let live = true;
    fetchPipelineVersions(pipelineId)
      .then((v) => {
        if (!live) return;
        setVersions(v);
        setError(null);
      })
      .catch((e: unknown) => {
        if (live) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      live = false;
    };
  }, [pipelineId, last.number, last.title, savedAt, listed?.modified, attempt]);

  return (
    <div className="pi-vers" data-testid="versions-tab">
      <div
        className="pi-draft"
        title="New Run and Triggers launch the last version"
        data-testid="versions-draft"
      >
        <CircleDashed size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
        <span className="pi-draft__name">Draft</span>
        <span className="d-mono ei-dim ei-trunc">
          {changes > 0 ? `${changesLabel(changes)} since v${last.number}` : `same as v${last.number}`}
        </span>
        <span className="pi-draft__people">
          <ParticipantStack participants={participants} size="sm" />
        </span>
      </div>
      {error ? (
        <div data-testid="versions-error">
          <EmptyState
            icon={CircleAlert}
            layout="row"
            action={
              <Button
                size="sm"
                onClick={() => {
                  // Back to the loading line while the list is read again.
                  setError(null);
                  setAttempt((n) => n + 1);
                }}
              >
                Retry
              </Button>
            }
          >
            <span title={error}>Versions unavailable</span>
          </EmptyState>
        </div>
      ) : versions == null ? (
        <EmptyState icon={Loader} layout="row" spin>
          Loading versions
        </EmptyState>
      ) : null}
      {versions?.map((version, index) => (
        <div
          key={version.number}
          className="pi-vrow"
          aria-current={viewing?.pipelineId === pipelineId && viewing.number === version.number ? true : undefined}
          title={`${versionLabel(version)} · ${fullDate(version.saved_at)}`}
          data-testid={`version-row-${version.number}`}
        >
          <span className="pi-vrow__n d-mono">v{version.number}</span>
          <span className="pi-vrow__title" data-ext={version.external || undefined}>
            {version.title}
          </span>
          {index === 0 && (
            <span className="pi-vrow__last" data-testid="version-last">
              <Chip mono>last</Chip>
            </span>
          )}
          <Author actor={version.author} external={version.external} />
          <span className="pi-vrow__ago d-mono">{shortSavedAgo(version.saved_at, now)}</span>
          <span
            className="pi-vrow__runs d-mono"
            title={version.runs === 0 ? "Never launched" : version.runs === 1 ? "1 run" : `${version.runs} runs`}
            data-testid={`version-runs-${version.number}`}
          >
            {version.runs > 0 && (
              <>
                <Play size={10} strokeWidth={1.5} aria-hidden />
                {version.runs}
              </>
            )}
          </span>
          <span className="pi-vrow__acts">
            <IconButton
              icon={Eye}
              size="sm"
              label={`View v${version.number}`}
              onClick={() => void openPipelineVersion(pipelineId, version.number)}
              data-testid={`version-view-${version.number}`}
            />
            <IconButton
              icon={RotateCcw}
              size="sm"
              label={`Restore v${version.number} into the draft`}
              onClick={() => void restoreIntoDraft(restoreVersion, pipelineId, version.number)}
              data-testid={`version-restore-${version.number}`}
            />
          </span>
        </div>
      ))}
    </div>
  );
}
