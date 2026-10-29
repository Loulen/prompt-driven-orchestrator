import { useEffect, useMemo, useRef, useState } from "react";
import { Bookmark, Check, ChevronDown, ChevronLeft, GitFork, SlidersHorizontal, TriangleAlert } from "lucide-react";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { AgentCombination } from "../model/settings";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import type { HarnessCatalog } from "../../harness/model/catalog";
import { findHarnessOption, effortOffer } from "../../harness/model/catalog";
import HarnessSelect from "../../harness/components/HarnessSelect";
import ModelPicker from "../../harness/components/ModelPicker";
import EffortPicker from "../../harness/components/EffortPicker";
import { combinationLabel, resolveAgentChoice } from "../model/agentProfiles";

const EMPTY: AgentCombination = { harness: "claude", model: null, effort: null };

export default function AgentControl({
  choice,
  onChange,
  profiles,
  catalog,
  inherited = EMPTY,
  allowInherit = true,
  label = "Agent",
  bare = false,
  testId = "agent-control",
}: {
  choice?: AgentChoice | null;
  onChange: (choice: AgentChoice) => void;
  profiles: AgentProfile[];
  catalog: HarnessCatalog;
  inherited?: AgentCombination;
  allowInherit?: boolean;
  label?: string;
  /** No caps label above: a Settings row names it. */
  bare?: boolean;
  testId?: string;
}) {
  const [open, setOpen] = useState(false);
  const [customPane, setCustomPane] = useState(false);
  const [custom, setCustom] = useState<AgentCombination>(() =>
    choice?.mode === "custom" ? choice : inherited,
  );
  const resolved = useMemo(
    () => resolveAgentChoice(choice, profiles, inherited),
    [choice, profiles, inherited],
  );
  const broken = resolved.brokenId != null;
  const modeLabel =
    broken ? resolved.brokenId
    : choice?.mode === "profile" ? resolved.profile?.name ?? "Profile"
    : choice?.mode === "custom" ? "Custom"
    : "Inherit";
  const Icon =
    broken ? TriangleAlert
    : choice?.mode === "profile" ? Bookmark
    : choice?.mode === "custom" ? SlidersHorizontal
    : GitFork;
  const harnessOption = findHarnessOption(catalog, custom.harness);
  // #798: the custom pane's effort offer is read against the model THIS pane has
  // selected (seeded from the choice when editing an existing custom, else from
  // the inherited combination it opens on) — per-model key authoritative
  // (`[]` included), missing key on the harness's global efforts; authoritative
  // ⇒ a stored effort outside the offer warns instead of silently persisting.
  const effort = effortOffer(harnessOption, custom.model);

  const rootRef = useRef<HTMLDivElement>(null);

  const close = () => {
    setOpen(false);
    setCustomPane(false);
  };

  // Dismiss on outside click and Escape (#686). The nested Harness/Model/Effort
  // pickers render through a portal, so a click or key inside one of them lands
  // outside `rootRef` — those events belong to the nested menu and are ignored.
  useEffect(() => {
    if (!open) return;
    const insidePortaledMenu = (target: EventTarget | null) =>
      target instanceof Element && target.closest('[data-slot="dropdown-menu-content"]') != null;
    const onMouseDown = (event: MouseEvent) => {
      const target = event.target;
      if (rootRef.current?.contains(target as Node) || insidePortaledMenu(target)) return;
      close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || insidePortaledMenu(event.target)) return;
      close();
    };
    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative" data-testid={`${testId}-root`}>
      {!bare && <span className="mb-1 block uppercase tracking-wider text-text-3 text-2xs">{label}</span>}
      <button
        type="button"
        data-testid={testId}
        // #824: what is chosen, machine-readable. The visible label is the
        // profile's NAME, which is renameable and sits next to a combination
        // summary in the same text node — nothing an observer can match on. The
        // id is fixed forever, so `profile:default` says exactly one thing.
        data-choice={
          choice?.mode === "profile"
            ? `profile:${choice.profile_id}`
            : (choice?.mode ?? "inherit")
        }
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={`flex w-full items-center gap-2 rounded border bg-surface-2 px-2 py-1.5 text-left ${
          broken ? "border-st-blocked text-st-blocked" : "border-line-strong text-text-2"
        }`}
      >
        <Icon size={11} className="shrink-0" />
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium text-xs">{modeLabel}</span>
          <span className="block truncate font-mono text-text-3 text-2xs">
            {broken ? `missing · ${combinationLabel(resolved.combination)}` : combinationLabel(resolved.combination)}
          </span>
        </span>
        <ChevronDown size={11} className="shrink-0 text-text-3" />
      </button>
      {broken && (
        <p className="mt-1 text-st-blocked text-2xs">
          Resolution continues at the next tier.
        </p>
      )}

      {open && (
        <div
          role="dialog"
          data-testid={`${testId}-popover`}
          className="absolute left-0 z-40 mt-1 w-full min-w-[280px] rounded border border-line-strong bg-surface-2 p-1.5 shadow-xl"
        >
          {customPane ? (
            <>
              <button
                type="button"
                onClick={() => setCustomPane(false)}
                className="mb-2 flex w-full items-center gap-1 border-b border-line px-1 pb-2 text-text-2"
              >
                <ChevronLeft size={12} /> Custom combination
              </button>
              <div className="space-y-2 px-1 pb-1">
                <label className="block text-text-3 text-xs">
                  Harness <span className="text-primary">required</span>
                  <HarnessSelect
                    value={custom.harness}
                    onChange={(harness) => setCustom({ harness: harness || "claude", model: null, effort: null })}
                    catalog={catalog}
                    inheritLabel="Choose a harness"
                    className="mt-1 w-full rounded border border-line-strong bg-surface-2 px-2 py-1.5"
                  />
                </label>
                <label className="block text-text-3 text-xs">
                  Model <span className="text-text-3">optional</span>
                  <ModelPicker
                    value={custom.model ?? null}
                    onChange={(model) => setCustom({ ...custom, model })}
                    models={harnessOption?.models ?? []}
                    contexts={harnessOption?.modelContexts}
                    testid={`${testId}-custom-model`}
                    subject={testId}
                  />
                </label>
                <label className="block text-text-3 text-xs">
                  Effort <span className="text-text-3">optional</span>
                  <EffortPicker
                    value={custom.effort ?? null}
                    onChange={(next) => setCustom({ ...custom, effort: next })}
                    efforts={effort.levels}
                    strict={effort.authoritative}
                    testid={`${testId}-custom-effort`}
                    disabled={!(harnessOption?.hasEffort ?? true)}
                  />
                </label>
                <button
                  type="button"
                  className="ml-auto block rounded bg-primary px-2 py-1 text-surface-0"
                  onClick={() => {
                    onChange({ mode: "custom", ...custom });
                    close();
                  }}
                >
                  Apply
                </button>
              </div>
            </>
          ) : (
            <>
              {allowInherit && (
                <ChoiceRow
                  testId={`${testId}-choice-inherit`}
                  name="Inherit"
                  summary={combinationLabel(inherited)}
                  selected={!choice || choice.mode === "inherit"}
                  onClick={() => {
                    onChange({ mode: "inherit" });
                    close();
                  }}
                />
              )}
              <div
                className="px-1 py-1 uppercase tracking-wider text-text-3 text-2xs"
                title="— = not set on the profile, so the harness default applies"
              >
                Profiles
                <span className="d-sr">: — = not set on the profile, so the harness default applies</span>
              </div>
              {profiles.map((profile) => (
                <ChoiceRow
                  key={profile.id}
                  // #824: one stable testid per profile, keyed on the id rather
                  // than the (renameable) name — a guided tour has to be able to
                  // aim at « Default », and the reserved id is the fixed part.
                  testId={`${testId}-choice-${profile.id}`}
                  name={profile.name}
                  summary={combinationLabel(profile)}
                  selected={choice?.mode === "profile" && choice.profile_id === profile.id}
                  onClick={() => {
                    onChange({ mode: "profile", profile_id: profile.id });
                    close();
                  }}
                />
              ))}
              <div className="mt-1 border-t border-line pt-1">
                <ChoiceRow
                  testId={`${testId}-choice-custom`}
                  name="Custom…"
                  summary="A combination for this tier only"
                  selected={choice?.mode === "custom"}
                  onClick={() => {
                    setCustom(choice?.mode === "custom" ? choice : inherited);
                    setCustomPane(true);
                  }}
                />
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function ChoiceRow({
  name,
  summary,
  selected,
  onClick,
  testId,
}: {
  name: string;
  summary: string;
  selected: boolean;
  onClick: () => void;
  testId?: string;
}) {
  return (
    <button type="button" data-testid={testId} onClick={onClick} className="relative block w-full rounded px-1.5 py-1 text-left hover:bg-hover">
      <span className={`block text-xs font-medium ${selected ? "text-primary" : "text-text"}`}>{name}</span>
      <span className="block font-mono text-text-3 text-2xs">{summary}</span>
      {selected && <Check size={12} className="absolute right-1.5 top-2 text-primary" />}
    </button>
  );
}
