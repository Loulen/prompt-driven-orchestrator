import { useState } from "react";
import { Bot, Lock, Pencil, Plus, Trash2 } from "lucide-react";
import ListEmpty from "../../shared/ui/leftList/ListEmpty";
import { deleteAgentProfile, fetchAgentProfileReferents } from "../api/agentProfiles";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import { relativeTime } from "../../platform/model/updateStatus";
import { ConfirmDialog, IconButton } from "../../shared/ui/kit";
import type { AgentProfileReferents } from "../model/settings";
import type { SettingsSection } from "../model/settingsCategories";
import Section, { SectionExtra } from "./settingsSurface/Section";
import AgentProfileModal from "./AgentProfileModal";
import { useListRefusals } from "../../shared/authorization/useRefusals";
import type { OperationQuestion } from "../../shared/authorization/api";

const CREATE_PROFILE: OperationQuestion[] = [{ resource: "agent_profile", verb: "create" }];
const profileId = (p: AgentProfile) => p.id;
const perProfile = (p: AgentProfile): OperationQuestion[] => [
  { resource: "agent_profile", verb: "update", target: p.id },
  { resource: "agent_profile", verb: "delete", target: p.id },
];

/** The column heads of the profiles table. */
const COLUMNS = ["Name", "Harness", "Model", "Effort", "Edited"];

/**
 * Settings › Agents › Agent profiles (#691): one row per profile in columns — name,
 * harness, model, effort, last edit — with Edit and Delete on hover. Each create / update /
 * delete is its own request — profiles are their own REST resource, not part of the
 * grouped `PUT /settings`, which is why the section is « saved at once » and the form's
 * Save never sends anything from here.
 *
 * The editor opens in a modal (#899) — from a row's name or pencil, or from the section's
 * « New profile » — the same modal for both. Delete asks first, naming how many live
 * references will resolve at the next tier.
 *
 * CORE-205: a gesture the caller may not make (`agent_profile` × create,
 * update, delete, as `POST /authorize` answers) stays, greyed, its tooltip
 * saying the refusal's sentence; the modal opens to read, its Save greyed.
 *
 * CORE-248: when the caller may not read the profiles (`refused`), the table gives way to
 * the refusal's sentence, with no New profile.
 */
export default function AgentProfilesPanel({
  section,
  profiles,
  refused = null,
  onChanged,
}: {
  section: SettingsSection;
  profiles: AgentProfile[];
  /** Why the caller may not read the profiles, `null` when it may. */
  refused?: string | null;
  onChanged: () => Promise<void>;
}) {
  // The profile open in the modal, `"new"` for New profile, `null` when closed.
  const [editing, setEditing] = useState<AgentProfile | "new" | null>(null);
  const [deleting, setDeleting] = useState<{ profile: AgentProfile; referents: AgentProfileReferents } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refusals = useListRefusals(profiles, profileId, perProfile, CREATE_PROFILE);
  const createRefused = refusals.global[0];
  const updateRefused = (id: string) => refusals.of(id)[0];
  const deleteRefused = (id: string) => refusals.of(id)[1];

  const inspectDelete = async (profile: AgentProfile) => {
    setError(null);
    try {
      setDeleting({ profile, referents: await fetchAgentProfileReferents(profile.id) });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to load referents");
    }
  };

  const confirmDelete = async () => {
    if (!deleting) return;
    try {
      await deleteAgentProfile(deleting.profile.id);
      setDeleting(null);
      await onChanged();
    } catch (cause) {
      setDeleting(null);
      setError(cause instanceof Error ? cause.message : "Failed to delete agent profile");
    }
  };

  const references = deleting ? referenceList(deleting.referents) : [];

  if (refused !== null) {
    return (
      <Section section={section}>
        <SectionExtra>
          <ListEmpty icon={Bot} title={refused} testId="agent-profiles-refused" />
        </SectionExtra>
      </Section>
    );
  }

  return (
    <Section
      section={section}
      tools={
        <IconButton
          icon={Plus}
          label="New profile"
          size="sm"
          disabledReason={createRefused}
          onClick={() => {
            setError(null);
            setEditing("new");
          }}
          data-testid="agent-profile-new"
        />
      }
    >
      <SectionExtra>
        <div role="table" aria-label="Agent profiles" data-testid="agent-profiles-panel">
          <div className="set-prow set-prow--head" role="row">
            {COLUMNS.map((column) => (
              <span key={column} role="columnheader">
                {column}
              </span>
            ))}
            <span role="columnheader">
              <span className="d-sr">Actions</span>
            </span>
          </div>
          {profiles.map((profile) => (
            <div key={profile.id} className="set-prow" role="row" data-testid={`agent-profile-row-${profile.id}`}>
              <span role="cell">
                <button
                  type="button"
                  className="set-prow__name"
                  title={profile.name}
                  onClick={() => {
                    setError(null);
                    setEditing(profile);
                  }}
                >
                  <span>{profile.name}</span>
                  {profile.id === "default" && (
                    <Lock size={11} strokeWidth={1.5} className="set-faint" aria-label="Built in" />
                  )}
                </button>
              </span>
              <span role="cell" title={profile.harness}>
                {profile.harness}
              </span>
              <span role="cell" className="set-mono" title={profile.model}>
                {profile.model || "–"}
              </span>
              <span role="cell">{profile.effort || "–"}</span>
              <span role="cell" className="set-mono set-faint" title={profile.updated_at}>
                {edited(profile.updated_at)}
              </span>
              <span role="cell" className="set-prow__acts">
                <IconButton
                  icon={Pencil}
                  label={`Edit ${profile.name}`}
                  size="sm"
                  disabledReason={updateRefused(profile.id)}
                  onClick={() => {
                    setError(null);
                    setEditing(profile);
                  }}
                />
                <IconButton
                  icon={Trash2}
                  label={`Delete ${profile.name}`}
                  size="sm"
                  disabled={profile.id === "default" && !deleteRefused(profile.id)}
                  disabledReason={deleteRefused(profile.id)}
                  onClick={() => void inspectDelete(profile)}
                />
              </span>
            </div>
          ))}
        </div>
        {error && (
          <p className="set-row__note set-row__note--failed" role="alert">
            {error}
          </p>
        )}
      </SectionExtra>

      {editing && (
        <AgentProfileModal
          profile={editing === "new" ? null : editing}
          profiles={profiles}
          saveRefused={editing === "new" ? createRefused : updateRefused(editing.id)}
          onClose={() => setEditing(null)}
          onSaved={onChanged}
        />
      )}
      {deleting && (
        <ConfirmDialog
          title={`Delete ${deleting.profile.name}?`}
          consequence={
            references.length === 0
              ? "No live reference"
              : references.length === 1
                ? "1 reference moves to the next tier"
                : `${references.length} references move to the next tier`
          }
          target={references.length > 0 ? references.join(", ") : undefined}
          actionLabel="Delete"
          danger
          onConfirm={() => void confirmDelete()}
          onClose={() => setDeleting(null)}
        />
      )}
    </Section>
  );
}

/** Who still names the profile, one entry each: the instance, Projects, Triggers, pipelines, Runs. */
function referenceList(referents: AgentProfileReferents): string[] {
  return [
    ...(referents.instance ? ["instance"] : []),
    ...referents.projects.map((item) => `project ${item.name}`),
    ...referents.triggers.map((item) => `trigger ${item.name}`),
    ...referents.pipelines.map((item) => `pipeline ${item.name}`),
    ...referents.runs.map((item) => `run ${item.name ?? item.run_id}`),
  ];
}

/** How long ago a profile was last edited, « – » when the daemon gives no date. */
function edited(at: string): string {
  return at && !Number.isNaN(Date.parse(at)) ? relativeTime(at) : "–";
}
