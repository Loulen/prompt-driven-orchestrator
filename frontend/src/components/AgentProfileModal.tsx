import { useEffect, useState } from "react";
import { createAgentProfile, updateAgentProfile } from "../api";
import type { AgentProfile } from "../types";
import ModelPicker from "./ModelPicker";
import EffortPicker from "./EffortPicker";
import HarnessSelect from "./HarnessSelect";
import { findHarnessOption, effortOffer } from "../lib/harness";
import { useHarnessCatalog } from "../hooks/useHarnessCatalog";

/**
 * The agent profile editor (#899): one modal for both a row's **Edit** pencil
 * (`profile` set, `Save profile`) and **New profile** (`profile` null, `Create`).
 * Each save is its own REST request — profiles are not part of the grouped
 * `PUT /settings`, so nothing here touches the Settings draft.
 *
 * Cancel, Escape and a backdrop click close it without sending anything; a
 * successful save closes it and lets the list refresh (`onSaved`).
 */
export default function AgentProfileModal({
  profile,
  profiles,
  onClose,
  onSaved,
}: {
  /** The profile being edited, or `null` to create a new one. */
  profile: AgentProfile | null;
  /** Every profile, for the case-insensitive unique-name check. */
  profiles: AgentProfile[];
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const [draft, setDraft] = useState(() => ({
    name: profile?.name ?? "",
    harness: profile?.harness ?? "",
    model: profile?.model ?? null,
    effort: profile?.effort ?? null,
  }));
  const [error, setError] = useState<string | null>(null);
  const catalog = useHarnessCatalog();
  const harnessOption = findHarnessOption(catalog, draft.harness);
  // #798: the effort offer is read against the model the form has selected — a
  // per-model key in the served `model_efforts` is authoritative for it
  // (`[]` included), a missing key retains the harness's global efforts. When
  // authoritative, a stored effort outside the offer renders as an unsupported
  // passthrough — warned, kept, never silently deleted (ADR-0001).
  const effort = effortOffer(harnessOption, draft.model);
  const nameKey = draft.name.trim().toLowerCase();
  const canSave =
    nameKey.length > 0 &&
    draft.harness !== "" &&
    !profiles.some((p) => p.id !== profile?.id && p.name.toLowerCase() === nameKey);

  // Settings is itself an overlay whose window-level Escape closes it: this
  // modal owns Escape while it is up (capture phase, so it runs before the
  // Settings shell). An open Harness/Model/Effort menu is portaled and closes
  // itself first — its keys are left alone.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const target = event.target;
      if (target instanceof Element && target.closest('[data-slot="dropdown-menu-content"]')) return;
      event.stopPropagation();
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const save = async () => {
    setError(null);
    try {
      if (profile) await updateAgentProfile(profile.id, draft);
      else await createAgentProfile(draft);
      onClose();
      await onSaved();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to save agent profile");
    }
  };

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60"
      data-testid="agent-profile-backdrop"
      onClick={(event) => {
        event.stopPropagation();
        onClose();
      }}
    >
      <div
        className="w-[440px] max-w-[90vw] space-y-2 rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
        role="dialog"
        aria-modal="true"
        aria-label={profile ? `Edit ${profile.name}` : "New agent profile"}
        data-testid="agent-profile-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="mb-1 font-medium text-fg">
          {profile ? "Edit agent profile" : "New agent profile"}
        </div>
        <label className="block text-fg-3" style={{ fontSize: 10 }}>
          Name
          <input
            autoFocus
            value={draft.name}
            onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            className="mt-1 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg"
          />
        </label>
        <label className="block text-fg-3" style={{ fontSize: 10 }}>
          Harness <span className="text-acc">required</span>
          <HarnessSelect
            value={draft.harness}
            onChange={(harness) => setDraft({ ...draft, harness, model: null, effort: null })}
            catalog={catalog}
            inheritLabel="Choose a harness…"
            data-testid="agent-profile-harness"
            className="mt-1 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5"
          />
        </label>
        {draft.harness && (
          <>
            <label className="block text-fg-3" style={{ fontSize: 10 }}>
              Model <span className="text-fg-4">optional</span>
              <ModelPicker
                value={draft.model}
                onChange={(model) => setDraft({ ...draft, model })}
                models={harnessOption?.models ?? []}
                contexts={harnessOption?.modelContexts}
                testid="agent-profile-model"
                subject={profile?.id ?? "new"}
              />
            </label>
            <label className="block text-fg-3" style={{ fontSize: 10 }}>
              Effort <span className="text-fg-4">optional</span>
              <EffortPicker
                value={draft.effort}
                onChange={(next) => setDraft({ ...draft, effort: next })}
                efforts={effort.levels}
                strict={effort.authoritative}
                testid="agent-profile-effort"
                disabled={!(harnessOption?.hasEffort ?? true)}
              />
            </label>
          </>
        )}
        {error && <p className="text-st-failed" style={{ fontSize: 10 }}>{error}</p>}
        <div className="flex justify-end gap-2 pt-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-line px-2 py-1 text-fg-3"
            data-testid="agent-profile-cancel"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!canSave}
            onClick={() => void save()}
            className="rounded bg-acc px-2 py-1 text-bg-1 disabled:opacity-40"
            data-testid="agent-profile-save"
          >
            {profile ? "Save profile" : "Create"}
          </button>
        </div>
      </div>
    </div>
  );
}
