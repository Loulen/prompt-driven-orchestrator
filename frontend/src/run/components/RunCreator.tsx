import { useEffect, useState } from "react";
import { User } from "lucide-react";
import type { Actor } from "../../shared/generated/Actor";
import type { RunState } from "../../shared/generated/RunState";
import ActorLabel from "../../shared/actors/ActorLabel";
import { fetchTrigger } from "../../trigger/api/triggers";

/**
 * Who created the Run (API 0.8), and for a fired Run the Trigger's last
 * editor, who answers for it, by their display name when an extension names
 * them (0.8). Nothing for a Run created before the core
 * recorded it.
 */
export default function RunCreator({ run }: { run: RunState }) {
  const triggerName = useTriggerName(run.created_by);
  if (!run.created_by) return null;
  return (
    <div className="mt-2 flex flex-wrap items-center gap-1.5 text-fg-3" style={{ fontSize: "10.5px" }} data-testid="run-created-by">
      <User size={11} className="shrink-0 text-fg-4" aria-hidden="true" />
      <span className="text-fg-4">Created by</span>
      <span
        className="rounded bg-bg-3 px-1.5 py-0.5 text-fg-2"
        title={run.created_by.kind === "trigger" ? run.created_by.trigger_id : undefined}
        data-testid="run-creator"
      >
        {triggerName ? `Trigger ${triggerName}` : <ActorLabel actor={run.created_by} />}
      </span>
      {run.trigger_editor && (
        <>
          <span className="text-fg-4">· last editor</span>
          <span className="rounded bg-bg-3 px-1.5 py-0.5 text-fg-2" data-testid="run-trigger-editor">
            <ActorLabel actor={run.trigger_editor} />
          </span>
        </>
      )}
    </div>
  );
}

/** The name of the Trigger that created the Run, once fetched; `null` for
 *  another creator, or a Trigger since deleted (the label keeps its id). */
function useTriggerName(creator: Actor | undefined): string | null {
  const triggerId = creator?.kind === "trigger" ? creator.trigger_id : null;
  const [name, setName] = useState<{ id: string; name: string } | null>(null);
  useEffect(() => {
    if (!triggerId) return;
    let live = true;
    fetchTrigger(triggerId)
      .then((t) => live && setName({ id: triggerId, name: t.name }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [triggerId]);
  return name && name.id === triggerId ? name.name : null;
}
