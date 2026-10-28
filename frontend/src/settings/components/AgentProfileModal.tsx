import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { createAgentProfile, updateAgentProfile } from "../api/agentProfiles";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import ModelPicker from "../../harness/components/ModelPicker";
import EffortPicker from "../../harness/components/EffortPicker";
import HarnessSelect from "../../harness/components/HarnessSelect";
import { findHarnessOption, effortOffer, modelCount, probedAgo } from "../../harness/model/catalog";
import { useHarnessCatalog, useRefreshModels } from "../../harness/store/useHarnessCatalog";

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
  // « ↻ »: probe the harness's models again; the catalogue above reads them
  // anew once the daemon answers, here and in every other browser.
  const { probing, failed, refresh } = useRefreshModels();
  const refreshing = probing.has(draft.harness);
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
      className="fixed inset-0 z-[60] flex items-center justify-center bg-scrim"
      data-testid="agent-profile-backdrop"
      onClick={(event) => {
        event.stopPropagation();
        onClose();
      }}
    >
      <div
        className="flex max-h-[calc(85vh/var(--app-zoom,1))] w-[440px] max-w-[calc(90vw/var(--app-zoom,1))] flex-col rounded-lg border border-line bg-surface-2"
        style={{ fontSize: "12px" }}
        role="dialog"
        aria-modal="true"
        aria-label={profile ? `Edit ${profile.name}` : "New agent profile"}
        data-testid="agent-profile-modal"
        onClick={(event) => event.stopPropagation()}
      >
        {/* #940: bounded to the window like New Run — only the fields scroll; the
            borders mark where the scrolling body is cut off. */}
        <div className="shrink-0 border-b border-line px-4 py-3 font-medium text-text">
          {profile ? "Edit agent profile" : "New agent profile"}
        </div>
        <div className="min-h-0 space-y-2 overflow-y-auto px-4 py-3" data-testid="agent-profile-body">
        <label className="block text-text-3" style={{ fontSize: 10 }}>
          Name
          <input
            autoFocus
            value={draft.name}
            onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            className="mt-1 w-full rounded border border-line-strong bg-surface-2 px-2 py-1.5 text-text"
          />
        </label>
        <label className="block text-text-3" style={{ fontSize: 10 }}>
          Harness <span className="text-primary">required</span>
          <HarnessSelect
            value={draft.harness}
            onChange={(harness) => setDraft({ ...draft, harness, model: null, effort: null })}
            catalog={catalog}
            inheritLabel="Choose a harness…"
            data-testid="agent-profile-harness"
            className="mt-1 w-full rounded border border-line-strong bg-surface-2 px-2 py-1.5"
          />
        </label>
        {draft.harness && (
          <>
            <div className="block text-text-3" style={{ fontSize: 10 }}>
              <div className="flex items-center gap-2">
                <span>
                  Model <span className="text-text-3">optional</span>
                </span>
                <span className="flex-1" />
                {failed[draft.harness] ? (
                  <span className="truncate text-st-failed" role="alert" data-testid="agent-profile-models-failed">
                    {failed[draft.harness]}
                  </span>
                ) : (
                  harnessOption?.installed && (
                    <span className="text-text-3" data-testid="agent-profile-models-probed">
                      {refreshing
                        ? "Probing…"
                        : `${modelCount(harnessOption.models.length)} · ${probedAgo(harnessOption.probedAt)}`}
                    </span>
                  )
                )}
                <button
                  type="button"
                  onClick={() => void refresh(draft.harness)}
                  disabled={refreshing || !(harnessOption?.installed ?? false)}
                  aria-label="Refresh models"
                  title="Probe the models of this harness again"
                  className="grid h-4 w-4 place-items-center rounded text-text-3 hover:text-text disabled:opacity-40"
                  data-testid="agent-profile-models-refresh"
                >
                  <RefreshCw size={11} className={refreshing ? "animate-spin" : undefined} />
                </button>
              </div>
              <div className="mt-1">
                <ModelPicker
                  value={draft.model}
                  onChange={(model) => setDraft({ ...draft, model })}
                  models={harnessOption?.models ?? []}
                  contexts={harnessOption?.modelContexts}
                  testid="agent-profile-model"
                  subject={profile?.id ?? "new"}
                />
              </div>
            </div>
            <label className="block text-text-3" style={{ fontSize: 10 }}>
              Effort <span className="text-text-3">optional</span>
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
        </div>
        <div className="shrink-0 border-t border-line px-4 py-3">
        {error && <p className="mb-2 text-st-failed" style={{ fontSize: 10 }}>{error}</p>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-line px-2 py-1 text-text-3"
            data-testid="agent-profile-cancel"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!canSave}
            onClick={() => void save()}
            className="rounded bg-primary px-2 py-1 text-surface-0 disabled:opacity-40"
            data-testid="agent-profile-save"
          >
            {profile ? "Save profile" : "Create"}
          </button>
        </div>
        </div>
      </div>
    </div>
  );
}
