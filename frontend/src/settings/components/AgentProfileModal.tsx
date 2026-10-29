import { useEffect, useState } from "react";
import { RefreshCw, X } from "lucide-react";
import { Button, Field, IconButton, TextInput } from "../../shared/ui/kit";
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

  const modelState = failed[draft.harness] ? (
    <span className="d-err set-ellipsis" role="alert" title={failed[draft.harness]} data-testid="agent-profile-models-failed">
      {failed[draft.harness]}
    </span>
  ) : (
    harnessOption?.installed && (
      <span className="set-faint text-xs" data-testid="agent-profile-models-probed">
        {refreshing ? "Probing…" : `${modelCount(harnessOption.models.length)} · ${probedAgo(harnessOption.probedAt)}`}
      </span>
    )
  );

  return (
    <div
      className="fixed inset-0 z-[60] grid place-items-center bg-scrim"
      data-testid="agent-profile-backdrop"
      onClick={(event) => {
        event.stopPropagation();
        onClose();
      }}
    >
      <div
        className="d-frame w-[500px] max-w-[calc(90vw/var(--app-zoom,1))] rounded-(--radius-ui) border border-line bg-surface-0"
        role="dialog"
        aria-modal="true"
        aria-label={profile ? `Edit ${profile.name}` : "New agent profile"}
        data-testid="agent-profile-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="d-frame__head">
          <h2 className="d-frame__title">{profile ? "Edit agent profile" : "New agent profile"}</h2>
          <span className="d-grow" />
          <IconButton icon={X} label="Close" kbd="Esc" onClick={onClose} />
        </header>
        {/* #940: bounded to the window like New Run — only the fields scroll. */}
        <div className="d-frame__body flex flex-col gap-2" data-testid="agent-profile-body">
          <Field label="Name">
            <TextInput
              autoFocus
              value={draft.name}
              onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            />
          </Field>
          <Field label="Harness" help="Required">
            <HarnessSelect
              value={draft.harness}
              onChange={(harness) => setDraft({ ...draft, harness, model: null, effort: null })}
              catalog={catalog}
              inheritLabel="Choose a harness…"
              data-testid="agent-profile-harness"
              className="d-input"
            />
          </Field>
          {draft.harness && (
            <>
              <div className="d-field">
                <span className="d-field__label" title="Optional">
                  Model
                </span>
                <div className="d-field__ctl flex-col items-stretch gap-1">
                  <ModelPicker
                    value={draft.model}
                    onChange={(model) => setDraft({ ...draft, model })}
                    models={harnessOption?.models ?? []}
                    contexts={harnessOption?.modelContexts}
                    testid="agent-profile-model"
                    subject={profile?.id ?? "new"}
                  />
                  <span className="set-line">
                    {modelState}
                    <span className="d-grow" />
                    <IconButton
                      icon={RefreshCw}
                      label="Refresh models"
                      size="sm"
                      className={refreshing ? "[&>svg]:animate-spin" : undefined}
                      onClick={() => void refresh(draft.harness)}
                      disabled={refreshing || !(harnessOption?.installed ?? false)}
                      data-testid="agent-profile-models-refresh"
                    />
                  </span>
                </div>
              </div>
              <div className="d-field">
                <span className="d-field__label" title="Optional">
                  Effort
                </span>
                <div className="d-field__ctl">
                  <EffortPicker
                    value={draft.effort}
                    onChange={(next) => setDraft({ ...draft, effort: next })}
                    efforts={effort.levels}
                    strict={effort.authoritative}
                    testid="agent-profile-effort"
                    disabled={!(harnessOption?.hasEffort ?? true)}
                  />
                </div>
              </div>
            </>
          )}
        </div>
        <footer className="d-frame__foot">
          <span className="d-frame__note">
            {error && (
              <span className="d-err set-ellipsis" role="alert" title={error}>
                {error}
              </span>
            )}
          </span>
          <div className="d-frame__actions">
            <Button onClick={onClose} data-testid="agent-profile-cancel">
              Cancel
            </Button>
            <Button tone="primary" disabled={!canSave} onClick={() => void save()} data-testid="agent-profile-save">
              {profile ? "Save profile" : "Create"}
            </Button>
          </div>
        </footer>
      </div>
    </div>
  );
}
