import { useState } from "react";
import { Lock, Pencil, Plus, Trash2 } from "lucide-react";
import {
  createAgentProfile,
  deleteAgentProfile,
  fetchAgentProfileReferents,
  updateAgentProfile,
} from "../api";
import type { AgentProfile, AgentProfileReferents } from "../types";
import ModelPicker from "./ModelPicker";
import EffortPicker from "./EffortPicker";
import HarnessSelect from "./HarnessSelect";
import { findHarnessOption } from "../lib/harness";
import { useHarnessCatalog } from "../hooks/useHarnessCatalog";

/**
 * Agent profiles editor, mounted inline in Settings › Agents › Agent profiles (#691). Each
 * create / update / delete is its own request — profiles are their own REST resource, not
 * part of the grouped `PUT /settings`, which is why the section says `saves as you go` and
 * the form's Save never sends anything from here.
 *
 * List-first: the editor stays folded until a row or **New profile** opens it, so a visit
 * to the Agents page reads as a list and never shows a second primary button next to the
 * footer's Save.
 */
export default function AgentProfilesPanel({
  profiles,
  onChanged,
}: {
  profiles: AgentProfile[];
  onChanged: () => Promise<void>;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState(() => ({
    name: "",
    harness: "",
    model: null as string | null,
    effort: null as string | null,
  }));
  const [referents, setReferents] = useState<AgentProfileReferents | null>(null);
  const [deleting, setDeleting] = useState<AgentProfile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const catalog = useHarnessCatalog();
  const selected = profiles.find((profile) => profile.id === selectedId) ?? null;
  const harnessOption = findHarnessOption(catalog, draft.harness);

  const edit = (profile: AgentProfile) => {
    setSelectedId(profile.id);
    setCreating(false);
    setError(null);
    setDraft({
      name: profile.name,
      harness: profile.harness,
      model: profile.model ?? null,
      effort: profile.effort ?? null,
    });
  };

  const save = async () => {
    setError(null);
    try {
      if (creating) {
        await createAgentProfile(draft);
      } else if (selected) {
        await updateAgentProfile(selected.id, draft);
      }
      setCreating(false);
      setDraft({ name: "", harness: "", model: null, effort: null });
      await onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to save agent profile");
    }
  };

  const inspectDelete = async (profile: AgentProfile) => {
    setError(null);
    try {
      setReferents(await fetchAgentProfileReferents(profile.id));
      setDeleting(profile);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to load referents");
    }
  };

  const confirmDelete = async () => {
    if (!deleting) return;
    try {
      await deleteAgentProfile(deleting.id);
      setDeleting(null);
      setReferents(null);
      setSelectedId(null);
      setDraft({ name: "", harness: "", model: null, effort: null });
      await onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to delete agent profile");
    }
  };

  if (deleting && referents) {
    const count =
      Number(referents.instance) + referents.projects.length + referents.triggers.length +
      referents.pipelines.length + referents.runs.length;
    return (
      <div className="flex flex-col p-4" data-testid="agent-profile-delete">
        <h3 className="font-semibold text-fg">Delete {deleting.name}?</h3>
        <p className="mt-2 text-fg-3" style={{ fontSize: 11 }}>
          These <strong>{count} live references</strong> will resolve at the next tier instead.
        </p>
        <div className="my-3 max-h-48 overflow-y-auto rounded border border-line bg-bg-3 p-2 font-mono text-fg-3" style={{ fontSize: 10 }}>
          {referents.instance && <div>INSTANCE — settings</div>}
          {referents.projects.map((item) => <div key={`p-${item.id}`}>PROJECT — {item.name}</div>)}
          {referents.triggers.map((item) => <div key={`t-${item.id}`}>TRIGGER — {item.name}</div>)}
          {referents.pipelines.map((item) => <div key={`l-${item.id}`}>PIPELINE — {item.name}</div>)}
          {referents.runs.map((item) => <div key={`r-${item.run_id}`}>RUN — {item.name ?? item.run_id}</div>)}
          {count === 0 && <div>No live references.</div>}
        </div>
        <p className="rounded border border-st-blocked/40 bg-st-blocked/10 p-2 text-fg-3" style={{ fontSize: 10 }}>
          Runs already started are untouched — their combination was frozen at spawn.
        </p>
        <div className="mt-auto flex justify-end gap-2 pt-4">
          <button onClick={() => setDeleting(null)} className="rounded border border-line px-3 py-1 text-fg-3">Cancel</button>
          <button onClick={confirmDelete} className="rounded bg-st-failed px-3 py-1 text-white">Delete anyway</button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col p-4" data-testid="agent-profiles-panel">
      <div className="space-y-1">
        {profiles.map((profile) => (
          <div
            key={profile.id}
            className={`flex items-center rounded px-2 py-2 ${selectedId === profile.id ? "bg-bg-3" : ""}`}
          >
            <button type="button" onClick={() => edit(profile)} className="min-w-0 flex-1 text-left">
              <span className="flex items-center gap-1 font-medium text-fg" style={{ fontSize: 11 }}>
                {profile.name} {profile.id === "default" && <Lock size={10} className="text-fg-4" />}
              </span>
              <span className="block font-mono text-fg-4" style={{ fontSize: 9.5 }}>
                {[profile.harness, profile.model || "—", profile.effort || "—"].join(" · ")}
              </span>
            </button>
            <Pencil size={11} className="mr-3 text-fg-4" />
            <button
              type="button"
              aria-label={`Delete ${profile.name}`}
              disabled={profile.id === "default"}
              onClick={() => void inspectDelete(profile)}
              className="text-fg-4 disabled:opacity-25"
            >
              <Trash2 size={12} />
            </button>
          </div>
        ))}
      </div>
      <button
        type="button"
        onClick={() => {
          setCreating(true);
          setSelectedId(null);
          setDraft({ name: "", harness: "", model: null, effort: null });
        }}
        className="mt-2 self-start rounded border border-line px-2 py-1 text-fg-2"
        data-testid="agent-profile-new"
      >
        <Plus size={11} className="mr-1 inline" /> New profile
      </button>

      {(creating || selected) && (
        <div className="mt-3 space-y-2 border-t border-line pt-3">
          <label className="block text-fg-3" style={{ fontSize: 10 }}>
            Name
            <input
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
                  subject={selectedId ?? "new"}
                />
              </label>
              <label className="block text-fg-3" style={{ fontSize: 10 }}>
                Effort <span className="text-fg-4">optional</span>
                <EffortPicker
                  value={draft.effort}
                  onChange={(effort) => setDraft({ ...draft, effort })}
                  efforts={harnessOption?.efforts ?? []}
                  testid="agent-profile-effort"
                  disabled={!(harnessOption?.hasEffort ?? true)}
                />
              </label>
            </>
          )}
          {error && <p className="text-st-failed" style={{ fontSize: 10 }}>{error}</p>}
          <div className="flex justify-end gap-2">
            <button onClick={() => { setSelectedId(null); setCreating(false); setDraft({ name: "", harness: "", model: null, effort: null }); }} className="rounded border border-line px-2 py-1 text-fg-3">Cancel</button>
            <button disabled={!draft.name.trim() || !draft.harness || profiles.some((p) => p.id !== selectedId && p.name.toLowerCase() === draft.name.trim().toLowerCase())} onClick={() => void save()} className="rounded bg-acc px-2 py-1 text-bg-1 disabled:opacity-40">
              {creating ? "Create" : "Save profile"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

