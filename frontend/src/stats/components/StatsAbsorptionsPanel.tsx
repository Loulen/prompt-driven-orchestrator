import { useEffect, useMemo, useState } from "react";
import { useRefusals } from "../../shared/authorization/useRefusals";
import { Combine } from "lucide-react";
import { fetchStatsAbsorptions, uncombineStatsMember } from "../api/absorptions";
import type { StatsAbsorption } from "../../shared/generated/StatsAbsorption";
import { day, MONO_DIMENSIONS } from "../model/statsAbsorption";
import { UncombineButton } from "./StatsAbsorption";
import { uncombineQuestion } from "../model/absorptionRights";

type Dimension = StatsAbsorption["dimension"];
type Member = StatsAbsorption["members"][number];

/** The list's groups, in the order Stats reads them (#892 adds Nodes and
 *  Models, #906 efforts and couples). */
const DIMENSIONS: { id: Dimension; label: string }[] = [
  { id: "pipeline", label: "Pipelines" },
  { id: "node", label: "Nodes" },
  { id: "model", label: "Models" },
  { id: "effort", label: "Efforts" },
  { id: "couple", label: "Couples" },
];

/** What a scoped absorption lives in, said in words (#892, #906): the Pipeline
 *  of a Node absorption, « Effort · <model> », « Couple · <Node> (<Pipeline>) ». */
function scopeLabel(absorption: StatsAbsorption): string | null {
  const name = absorption.scope_name || null;
  if (!name) return null;
  if (absorption.dimension === "node") return `in ${name}`;
  if (absorption.dimension === "effort") return `Effort · ${name}`;
  if (absorption.dimension === "couple") return `Couple · ${name}`;
  return null;
}

function originNote(member: Member): string {
  const how = member.origin === "rename" ? "renamed" : "combined in Stats";
  const when = day(member.created_at);
  return when ? `${how} on ${when}` : how;
}

/**
 * Settings › General › Stats absorptions (#891, ADR-0077): every absorption of
 * the instance, including those whose absorbent has not run in the period Stats
 * shows — which Stats itself cannot list. Each ✕ is its own request, like the
 * members list in Stats: the section `saves as you go`, the form's Save never
 * sends anything from here. Names only, never a key.
 */
export default function StatsAbsorptionsPanel({ active }: { active: boolean }) {
  const [absorptions, setAbsorptions] = useState<StatsAbsorption[] | null>(
    null,
  );
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // CORE-205: the ✕ the caller may not use, greyed with why (`stats_absorption` × delete on the member).
  const allMembers = useMemo(() => (absorptions ?? []).flatMap((a) => a.members), [absorptions]);
  const questions = useMemo(() => allMembers.map(uncombineQuestion), [allMembers]);
  const refusals = useRefusals(questions);
  const uncombineRefused = (member: { key: string }) => refusals[allMembers.findIndex((m) => m.key === member.key)];

  // Read at each open of Settings: Stats or a rename may have written since.
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    fetchStatsAbsorptions()
      .then((list) => {
        if (cancelled) return;
        setAbsorptions(list.absorptions);
        setLoadError(null);
      })
      .catch((cause) => {
        if (!cancelled)
          setLoadError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [active]);

  const uncombine = async (absorption: StatsAbsorption, member: Member) => {
    const busy = `${absorption.dimension}\u0000${absorption.scope}\u0000${member.key}`;
    setBusyKey(busy);
    setError(null);
    try {
      const list = await uncombineStatsMember(
        absorption.dimension,
        absorption.scope,
        member.key,
      );
      setAbsorptions(list.absorptions);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusyKey(null);
    }
  };

  if (loadError && !absorptions) {
    return (
      <div className="sx-err" data-testid="stats-absorptions-error">
        {loadError}
      </div>
    );
  }
  if (!absorptions) {
    return (
      <div className="d-meta">Loading…</div>
    );
  }
  if (absorptions.length === 0) {
    return (
      <div
        className="sx-empty flex items-center justify-center gap-2"
        data-testid="stats-absorptions-empty"
        title="In Stats, Ctrl/Cmd-click two rows and choose Combine; renaming a pipeline in the library combines its old and new name by itself."
      >
        <Combine size={14} strokeWidth={1.5} aria-hidden="true" />
        Nothing combined
      </div>
    );
  }

  return (
    <div
      className="flex flex-col gap-4 text-sm"
      data-testid="stats-absorptions-panel"
    >
      {DIMENSIONS.map(({ id, label }) => {
        const group = absorptions.filter(
          (absorption) => absorption.dimension === id,
        );
        if (group.length === 0) return null;
        return (
          <div
            key={id}
            className="flex flex-col gap-2"
            data-testid={`stats-absorptions-${id}`}
          >
            <div className="d-caps">{label}</div>
            {group.map((absorption) => (
              <ul
                key={`${absorption.scope}\u0000${absorption.absorbent.key}`}
                className="sx-mlist"
                data-testid="stats-absorption"
              >
                <li className="sx-mlist__row">
                  <span className="flex min-w-0 flex-1 items-center gap-2">
                    <Combine
                      size={12}
                      strokeWidth={1.5}
                      className="sx-acc"
                      aria-hidden="true"
                    />
                    <span
                      className={`truncate text-text ${MONO_DIMENSIONS.has(id) ? "font-mono" : ""}`}
                      title={absorption.absorbent.name}
                      data-testid="stats-absorption-absorbent"
                    >
                      {absorption.absorbent.name}
                    </span>
                    {scopeLabel(absorption) && (
                      // A scoped absorption lives in one Pipeline, model or
                      // Node: say which.
                      <span
                        className="d-meta truncate"
                        data-testid="stats-absorption-scope"
                      >
                        {scopeLabel(absorption)}
                      </span>
                    )}
                  </span>
                  <span className="d-meta shrink-0">keeps its name</span>
                </li>
                {absorption.members.map((member) => (
                  <li
                    key={member.key}
                    className="sx-mlist__row"
                    data-testid="stats-absorption-member"
                  >
                    <span className="sx-mlist__two">
                      <span
                        className={`sx-mlist__name ${MONO_DIMENSIONS.has(id) ? "font-mono" : ""}`}
                        title={member.name}
                      >
                        {member.name}
                      </span>
                      <span
                        className="d-meta"
                        data-testid="stats-absorption-origin"
                      >
                        {originNote(member)}
                      </span>
                    </span>
                    <UncombineButton
                      name={member.name}
                      disabled={busyKey !== null}
                      disabledReason={uncombineRefused(member)}
                      onClick={() => void uncombine(absorption, member)}
                    />
                  </li>
                ))}
              </ul>
            ))}
          </div>
        );
      })}
      {error && (
        <div className="sx-err" data-testid="stats-absorptions-error">
          {error}
        </div>
      )}
    </div>
  );
}
