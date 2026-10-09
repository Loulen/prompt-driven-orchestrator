import { useMemo } from "react";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { RunPeopleContext } from "../../../shared/extensions/runPeople";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { useRunPresences } from "../../store/presences";

/**
 * The people of a Run's row (`run/item/people`, API 0.12), after the
 * pipeline's name: what the extensions draw of who has the Run open, live.
 * The core draws no person itself: nothing at all without a contribution.
 */
export default function RunRowPeople({ run }: { run: RunListEntry }) {
  const presences = useRunPresences(run.run_id);
  const extra = useMemo(
    () => ({
      entity: {
        kind: "run" as const,
        id: run.run_id,
        name: run.name ?? null,
        attributes: run.attributes ?? {},
        created_by: run.created_by ?? null,
      },
      presences,
    }),
    [run.run_id, run.name, run.attributes, run.created_by, presences],
  );
  return (
    <ExtensionDecorations<RunPeopleContext>
      location="run/item/people"
      extra={extra}
      className="shrink-0 font-sans"
    />
  );
}
