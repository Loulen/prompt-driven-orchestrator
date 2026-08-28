import { useMemo, useState } from "react";
import { Bookmark, Check, ChevronDown, ChevronLeft, GitFork, SlidersHorizontal, TriangleAlert } from "lucide-react";
import type { AgentChoice, AgentCombination, AgentProfile } from "../types";
import type { HarnessCatalog } from "../lib/harness";
import { findHarnessOption } from "../lib/harness";
import HarnessSelect from "./HarnessSelect";
import ModelPicker from "./ModelPicker";
import EffortPicker from "./EffortPicker";
import { combinationLabel, resolveAgentChoice } from "../lib/agentProfiles";

const EMPTY: AgentCombination = { harness: "claude", model: null, effort: null };

export default function AgentControl({
  choice,
  onChange,
  profiles,
  catalog,
  inherited = EMPTY,
  allowInherit = true,
  label = "Agent",
  testId = "agent-control",
}: {
  choice?: AgentChoice | null;
  onChange: (choice: AgentChoice) => void;
  profiles: AgentProfile[];
  catalog: HarnessCatalog;
  inherited?: AgentCombination;
  allowInherit?: boolean;
  label?: string;
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

  const close = () => {
    setOpen(false);
    setCustomPane(false);
  };

  return (
    <div className="relative" data-testid={`${testId}-root`}>
      <span className="mb-1 block uppercase tracking-wider text-fg-4" style={{ fontSize: 9 }}>{label}</span>
      <button
        type="button"
        data-testid={testId}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={`flex w-full items-center gap-2 rounded border bg-bg-3 px-2 py-1.5 text-left ${
          broken ? "border-st-blocked text-st-blocked" : "border-line-strong text-fg-2"
        }`}
      >
        <Icon size={11} className="shrink-0" />
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium" style={{ fontSize: 10.5 }}>{modeLabel}</span>
          <span className="block truncate font-mono text-fg-4" style={{ fontSize: 9.5 }}>
            {broken ? `missing · ${combinationLabel(resolved.combination)}` : combinationLabel(resolved.combination)}
          </span>
        </span>
        <ChevronDown size={11} className="shrink-0 text-fg-4" />
      </button>
      {broken && (
        <p className="mt-1 text-st-blocked" style={{ fontSize: 9.5 }}>
          Resolution continues at the next tier.
        </p>
      )}

      {open && (
        <div
          role="dialog"
          data-testid={`${testId}-popover`}
          className="absolute left-0 z-40 mt-1 w-full min-w-[280px] rounded border border-line-strong bg-bg-4 p-1.5 shadow-xl"
        >
          {customPane ? (
            <>
              <button
                type="button"
                onClick={() => setCustomPane(false)}
                className="mb-2 flex w-full items-center gap-1 border-b border-line px-1 pb-2 text-fg-2"
              >
                <ChevronLeft size={12} /> Custom combination
              </button>
              <div className="space-y-2 px-1 pb-1">
                <label className="block text-fg-3" style={{ fontSize: 10 }}>
                  Harness <span className="text-acc">required</span>
                  <HarnessSelect
                    value={custom.harness}
                    onChange={(harness) => setCustom({ harness: harness || "claude", model: null, effort: null })}
                    catalog={catalog}
                    inheritLabel="Choose a harness"
                    className="mt-1 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5"
                  />
                </label>
                <label className="block text-fg-3" style={{ fontSize: 10 }}>
                  Model <span className="text-fg-4">optional</span>
                  <ModelPicker
                    value={custom.model ?? null}
                    onChange={(model) => setCustom({ ...custom, model })}
                    models={harnessOption?.models ?? []}
                    testid={`${testId}-custom-model`}
                    subject={testId}
                  />
                </label>
                <label className="block text-fg-3" style={{ fontSize: 10 }}>
                  Effort <span className="text-fg-4">optional</span>
                  <EffortPicker
                    value={custom.effort ?? null}
                    onChange={(effort) => setCustom({ ...custom, effort })}
                    efforts={harnessOption?.efforts ?? []}
                    testid={`${testId}-custom-effort`}
                    disabled={!(harnessOption?.hasEffort ?? true)}
                  />
                </label>
                <button
                  type="button"
                  className="ml-auto block rounded bg-acc px-2 py-1 text-bg-1"
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
                  name="Inherit"
                  summary={combinationLabel(inherited)}
                  selected={!choice || choice.mode === "inherit"}
                  onClick={() => {
                    onChange({ mode: "inherit" });
                    close();
                  }}
                />
              )}
              <div className="px-1 py-1 uppercase tracking-wider text-fg-4" style={{ fontSize: 9 }}>Profiles</div>
              {profiles.map((profile) => (
                <ChoiceRow
                  key={profile.id}
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
                  name="Custom…"
                  summary="A combination for this tier only"
                  selected={choice?.mode === "custom"}
                  onClick={() => {
                    setCustom(choice?.mode === "custom" ? choice : inherited);
                    setCustomPane(true);
                  }}
                />
              </div>
              <p className="border-t border-line px-1 pt-1 text-fg-4" style={{ fontSize: 9 }}>
                — = not set on the profile, so the harness default applies.
              </p>
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
}: {
  name: string;
  summary: string;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button type="button" onClick={onClick} className="relative block w-full rounded px-1.5 py-1 text-left hover:bg-bg-3">
      <span className={`block font-medium ${selected ? "text-acc" : "text-fg"}`} style={{ fontSize: 10.5 }}>{name}</span>
      <span className="block font-mono text-fg-4" style={{ fontSize: 9 }}>{summary}</span>
      {selected && <Check size={12} className="absolute right-1.5 top-2 text-acc" />}
    </button>
  );
}
