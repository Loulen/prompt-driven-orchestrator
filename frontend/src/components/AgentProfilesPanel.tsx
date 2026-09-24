import { useState } from "react";
import { Lock, Pencil, Plus, Trash2 } from "lucide-react";
import { deleteAgentProfile, fetchAgentProfileReferents } from "../api";
import type { AgentProfile, AgentProfileReferents } from "../types";
import AgentProfileModal from "./AgentProfileModal";

/**
 * Agent profiles list, mounted inline in Settings › Agents › Agent profiles (#691). Each
 * create / update / delete is its own request — profiles are their own REST resource, not
 * part of the grouped `PUT /settings`, which is why the section says `saves as you go` and
 * the form's Save never sends anything from here.
 *
 * List-first: the list stays inline, the editor opens in a modal (#899) — only from a row's
 * **Edit** pencil or from **New profile**, the same modal for both. A row itself is plain
 * content, so a visit to the Agents page never shows a second primary button next to the
 * footer's Save.
 */
export default function AgentProfilesPanel({
  profiles,
  onChanged,
}: {
  profiles: AgentProfile[];
  onChanged: () => Promise<void>;
}) {
  // The profile open in the modal, `"new"` for New profile, `null` when closed.
  const [editing, setEditing] = useState<AgentProfile | "new" | null>(null);
  const [referents, setReferents] = useState<AgentProfileReferents | null>(null);
  const [deleting, setDeleting] = useState<AgentProfile | null>(null);
  const [error, setError] = useState<string | null>(null);

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
          <div key={profile.id} className="flex items-center rounded px-2 py-2">
            <div className="min-w-0 flex-1">
              <span className="flex items-center gap-1 font-medium text-fg" style={{ fontSize: 11 }}>
                {profile.name} {profile.id === "default" && <Lock size={10} className="text-fg-4" />}
              </span>
              <span className="block font-mono text-fg-4" style={{ fontSize: 9.5 }}>
                {[profile.harness, profile.model || "—", profile.effort || "—"].join(" · ")}
              </span>
            </div>
            <button
              type="button"
              aria-label={`Edit ${profile.name}`}
              onClick={() => {
                setError(null);
                setEditing(profile);
              }}
              className="mr-3 text-fg-4 hover:text-fg"
            >
              <Pencil size={11} />
            </button>
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
          setError(null);
          setEditing("new");
        }}
        className="mt-2 self-start rounded border border-line px-2 py-1 text-fg-2"
        data-testid="agent-profile-new"
      >
        <Plus size={11} className="mr-1 inline" /> New profile
      </button>
      {error && <p className="mt-2 text-st-failed" style={{ fontSize: 10 }}>{error}</p>}

      {editing && (
        <AgentProfileModal
          profile={editing === "new" ? null : editing}
          profiles={profiles}
          onClose={() => setEditing(null)}
          onSaved={onChanged}
        />
      )}
    </div>
  );
}
