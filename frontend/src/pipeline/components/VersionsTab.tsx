import { useEffect, useState } from "react";
import { CircleDashed, Eye, RotateCcw } from "lucide-react";
import type { PipelineVersionEntry } from "../../shared/generated/PipelineVersionEntry";
import type { Actor } from "../../shared/generated/Actor";
import { actorLabel } from "../../shared/lib/actor";
import { ActorAvatar } from "../../shared/actors/ActorAvatar";
import { useActorDisplay } from "../../shared/actors/store";
import { toast } from "../../shared/ui/toast";
import { fetchPipelineVersions } from "../api/versions";
import { changesLabel, lastVersionOf, savedAgo } from "../model/versions";
import { useEditStore } from "../store/editStore";
import { ParticipantStack } from "./ParticipantAvatar";

/** Who saved a version: its avatar, or « file » when nobody in PDO did. */
function Author({ actor }: { actor: Actor | null }) {
  const display = useActorDisplay(actor);
  if (!actor) return <span className="font-mono">file</span>;
  const name = display?.name ?? actorLabel(actor);
  return (
    <span title={name} className="inline-flex items-center">
      <ActorAvatar display={display ?? { name, avatar_url: null }} size={12} />
    </span>
  );
}

function runsLabel(runs: number): string | null {
  if (runs === 0) return null;
  return runs === 1 ? "1 run" : `${runs} runs`;
}

/**
 * The « Versions » tab of a pipeline (ADR-0088): its draft first, then every
 * version a Save froze, newest first. « View » opens a version read only;
 * « Restore » makes it the content of the draft, for every participant, as
 * one change Undo takes back. Neither creates a version.
 */
export default function VersionsTab({ pipelineId }: { pipelineId: string }) {
  const pipelines = useEditStore((s) => s.pipelines);
  const tabDraft = useEditStore((s) => s.openTabs.find((t) => t.id === pipelineId)?.draft);
  const openPipelineVersion = useEditStore((s) => s.openPipelineVersion);
  const restoreVersion = useEditStore((s) => s.restoreVersion);
  const last = lastVersionOf(pipelines, pipelineId);
  const listed = pipelines.find((p) => p.id === pipelineId);
  const changes = tabDraft?.changes ?? listed?.draft?.changes ?? 0;
  const participants = listed?.draft?.participants ?? tabDraft?.participants ?? [];
  const [versions, setVersions] = useState<PipelineVersionEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

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
  }, [pipelineId, last.number, last.title]);

  const restore = async (number: number) => {
    try {
      await restoreVersion(pipelineId, number);
      toast(`v${number} restored into the draft. Undo takes it back.`);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    }
  };

  return (
    <div className="flex flex-col" style={{ fontSize: "11.5px" }} data-testid="versions-tab">
      <p className="px-3 py-2 text-fg-4" style={{ fontSize: "10.5px" }}>
        New Run and Triggers launch the last version. The draft is never launched.
      </p>
      <div className="flex items-center gap-2 bg-bg-3/60 px-3 py-2" data-testid="versions-draft">
        <CircleDashed size={11} className="shrink-0 text-fg-4" />
        <span className="font-medium text-fg">Draft</span>
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {changes > 0 ? `${changesLabel(changes)} since v${last.number}` : `same as v${last.number}`}
        </span>
        <span className="ml-auto">
          <ParticipantStack participants={participants} size={14} />
        </span>
      </div>
      {error && (
        <p className="px-3 py-2 text-st-failed" data-testid="versions-error">
          {error}
        </p>
      )}
      {versions?.map((version, index) => {
        const runs = runsLabel(version.runs);
        return (
          <div
            key={version.number}
            className="group flex items-center gap-3 border-b border-line px-3 py-2 hover:bg-bg-3"
            data-testid={`version-row-${version.number}`}
          >
            <span className="w-6 shrink-0 font-mono text-fg-3">v{version.number}</span>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className={`truncate ${version.external ? "italic text-fg-2" : "text-fg"}`}>{version.title}</span>
                {index === 0 && (
                  <span
                    className="rounded bg-st-await-bg px-1 font-mono text-st-await"
                    style={{ fontSize: "9px" }}
                    data-testid="version-last"
                  >
                    last
                  </span>
                )}
              </div>
              <div className="mt-0.5 flex items-center gap-1.5 text-fg-4" style={{ fontSize: "10px" }}>
                <Author actor={version.author} />
                <span>{savedAgo(version.saved_at)}</span>
                {runs && <span>· {runs}</span>}
              </div>
            </div>
            <div className="hidden shrink-0 items-center gap-1 group-hover:flex group-focus-within:flex">
              <button
                onClick={() => void openPipelineVersion(pipelineId, version.number)}
                className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-2 px-1.5 py-0.5 text-fg-2 hover:bg-bg-4 hover:text-fg"
                style={{ fontSize: "10.5px" }}
                data-testid={`version-view-${version.number}`}
              >
                <Eye size={10} />
                View
              </button>
              <button
                onClick={() => void restore(version.number)}
                className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-2 px-1.5 py-0.5 text-fg-2 hover:bg-bg-4 hover:text-fg"
                style={{ fontSize: "10.5px" }}
                data-testid={`version-restore-${version.number}`}
              >
                <RotateCcw size={10} />
                Restore
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
