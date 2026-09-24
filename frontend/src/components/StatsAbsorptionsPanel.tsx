import { useEffect, useState } from "react";
import { Combine } from "lucide-react";
import { fetchStatsAbsorptions, uncombineStatsMember } from "../api";
import type { StatsAbsorption } from "../types";
import { day, MONO_DIMENSIONS } from "../lib/statsAbsorption";
import { UncombineButton } from "./StatsAbsorption";

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
      <div
        className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
        style={{ fontSize: "11px" }}
        data-testid="stats-absorptions-error"
      >
        {loadError}
      </div>
    );
  }
  if (!absorptions) {
    return (
      <div className="text-fg-4" style={{ fontSize: "11px" }}>
        Loading…
      </div>
    );
  }
  if (absorptions.length === 0) {
    return (
      <div
        className="rounded-md border border-dashed border-line px-3 py-3 text-fg-4"
        style={{ fontSize: "11px" }}
        data-testid="stats-absorptions-empty"
      >
        Nothing is combined. In Stats, Ctrl/Cmd-click two rows and choose
        Combine; renaming a pipeline in the library combines its old and new
        name by itself.
      </div>
    );
  }

  return (
    <div
      className="flex flex-col gap-4"
      data-testid="stats-absorptions-panel"
      style={{ fontSize: "12px" }}
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
            <div
              className="uppercase tracking-wide text-fg-4"
              style={{ fontSize: "9.5px" }}
            >
              {label}
            </div>
            {group.map((absorption) => (
              <ul
                key={`${absorption.scope}\u0000${absorption.absorbent.key}`}
                className="rounded-md border border-line"
                data-testid="stats-absorption"
              >
                <li className="flex items-center justify-between gap-3 px-3 py-2">
                  <span className="flex min-w-0 items-center gap-2">
                    <Combine
                      size={12}
                      className="shrink-0 text-acc"
                      aria-hidden="true"
                    />
                    <span
                      className={`truncate text-fg ${MONO_DIMENSIONS.has(id) ? "font-mono" : ""}`}
                      data-testid="stats-absorption-absorbent"
                    >
                      {absorption.absorbent.name}
                    </span>
                    {scopeLabel(absorption) && (
                      // A scoped absorption lives in one Pipeline, model or
                      // Node: say which.
                      <span
                        className="truncate text-fg-4"
                        style={{ fontSize: "10.5px" }}
                        data-testid="stats-absorption-scope"
                      >
                        {scopeLabel(absorption)}
                      </span>
                    )}
                  </span>
                  <span
                    className="shrink-0 text-fg-4"
                    style={{ fontSize: "10.5px" }}
                  >
                    keeps its name
                  </span>
                </li>
                {absorption.members.map((member) => (
                  <li
                    key={member.key}
                    className="flex items-center justify-between gap-3 border-t border-line px-3 py-2"
                    data-testid="stats-absorption-member"
                  >
                    <span className="min-w-0">
                      <span
                        className={`block truncate text-fg ${MONO_DIMENSIONS.has(id) ? "font-mono" : ""}`}
                      >
                        {member.name}
                      </span>
                      <span
                        className="block text-fg-4"
                        style={{ fontSize: "10.5px" }}
                        data-testid="stats-absorption-origin"
                      >
                        {originNote(member)}
                      </span>
                    </span>
                    <UncombineButton
                      name={member.name}
                      disabled={busyKey !== null}
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
        <div
          className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
          style={{ fontSize: "11px" }}
          data-testid="stats-absorptions-error"
        >
          {error}
        </div>
      )}
      <p className="text-fg-4" style={{ fontSize: "10.5px" }}>
        Uncombining a name brings its own row back in Stats. Nothing is
        rewritten: Stats only reads the rows differently.
      </p>
    </div>
  );
}
