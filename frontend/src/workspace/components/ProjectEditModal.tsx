import { useMemo, useState } from "react";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { Project } from "../model/project";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { addProjectMember, createProject, removeProjectMember, updateProject } from "../api/projects";
import { ApiError } from "../../shared/http/client";
import { useHarnessCatalog } from "../../harness/store/useHarnessCatalog";
import { useAgentProfiles } from "../../hooks/useAgentProfiles";
import AgentControl from "../../components/AgentControl";
import HarnessSelect from "../../harness/components/HarnessSelect";
import PersistedProvisioningEditor from "./PersistedProvisioningEditor";
import SkillSelector from "../../skill/components/SkillSelector";
import { useSkillBank } from "../../skill/store/useSkillBank";
import { announceSkillTiersChanged, useSkillTiers } from "../../skill/store/useSkillTiers";

/**
 * The group-header pencil (#552, ADR-0046): name a Projet (or rename an existing
 * one), pick which repo paths are its members, and optionally pose the harness it
 * carries. Naming a group is what **materialises** the Projet — nothing is seeded
 * until this saves (ADR-0046).
 *
 * A candidate repo already owned by **another** Projet is shown disabled with the
 * owner's name (the "refus nommant le propriétaire" surfaced up front); the daemon
 * refuses it too (409), which we also catch defensively.
 *
 * Membership is compared **verbatim** (ADR-0033): the candidate list is the exact
 * effective-repo paths the surrounding list groups by, never canonicalised.
 */

export default function ProjectEditModal({
  initialProject,
  initialName,
  initialMemberPaths,
  availableRepos,
  projects,
  onClose,
  onSaved,
}: {
  /** The Projet being edited, or `null` when the pencil creates a fresh one. */
  initialProject: Project | null;
  /** Pre-filled name: the Projet's name, or the group's derived label. */
  initialName: string;
  /** Pre-checked member paths: the Projet's members, or the group's own path. */
  initialMemberPaths: string[];
  /** Candidate repo paths to attach (the surrounding list's distinct repos). */
  availableRepos: string[];
  /** All Projets, to mark a candidate owned by another Projet as disabled. */
  projects: Project[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(initialName);
  // The Projet this modal created, when a later step of the save failed: a retry
  // edits it instead of naming a duplicate.
  const [created, setCreated] = useState<Project | null>(null);
  const editing = initialProject ?? created;
  const [harness, setHarness] = useState<string>(initialProject?.harness ?? "");
  const [agentChoice, setAgentChoice] = useState<AgentChoice>(
    initialProject?.agent_choice ?? { mode: "inherit" },
  );
  // #669/ADR-0062: the Projet tier of the skills selection; inherits the instance.
  const [skills, setSkills] = useState<SkillRef[]>(initialProject?.skills ?? []);
  const skillsChanged =
    skills.map((skill) => skill.id).join("\u0000") !==
    (editing?.skills ?? []).map((skill) => skill.id).join("\u0000");
  const { bank: skillBank } = useSkillBank();
  const { instance: instanceSkills } = useSkillTiers(null);
  const [checked, setChecked] = useState<Set<string>>(
    () => new Set(initialMemberPaths),
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [provisioningOpen, setProvisioningOpen] = useState(false);
  // #586: the harness options, dynamic from `/settings` (floor ∪ descriptors,
  // each installed/not). The modal holds no settings of its own, so it fetches.
  const harnessCatalog = useHarnessCatalog();
  const { profiles: agentProfiles } = useAgentProfiles();

  // path → name of the Projet that OWNS it, excluding the one being edited. A
  // candidate owned elsewhere cannot be attached here (AC: at most one Projet).
  const ownerElsewhere = useMemo(() => {
    const map = new Map<string, string>();
    for (const p of projects) {
      if (editing && p.id === editing.id) continue;
      for (const path of p.members) map.set(path, p.name);
    }
    return map;
  }, [projects, editing]);

  // The candidate universe: the surrounding list's repos, plus any existing
  // members (so a member currently filtered out of the list can still be unchecked).
  const candidates = useMemo(() => {
    const set = new Set<string>(availableRepos);
    for (const m of initialMemberPaths) set.add(m);
    return [...set].sort();
  }, [availableRepos, initialMemberPaths]);

  const trimmedName = name.trim();
  const canSave = trimmedName.length > 0 && !submitting;

  function toggle(path: string) {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  async function handleSave() {
    if (!canSave) return;
    setSubmitting(true);
    setError(null);
    let createdNow = false;
    try {
      // Materialise (or reuse) the Projet, then reconcile its name, harness and
      // members. A fresh Projet is named with its members in one creation, so an
      // extension refusing only updates still lets the group be named.
      let projectId: string;
      let current: Set<string>;
      if (editing) {
        projectId = editing.id;
        current = new Set(editing.members);
        await updateProject(projectId, {
          name: trimmedName,
          // Empty select → clear the harness (null); a value sets it.
          harness: harness ? harness : null,
          ...(agentChoice.mode === "inherit" ? {} : { agent_choice: agentChoice }),
          // #669: replaced wholesale when it changed; an empty list clears the tier.
          ...(skillsChanged ? { skills } : {}),
        });
      } else {
        const project = await createProject(trimmedName, [...checked]);
        projectId = project.id;
        current = new Set(project.members);
        setCreated(project);
        createdNow = true;
        const settings = {
          ...(harness ? { harness } : {}),
          ...(agentChoice.mode === "inherit" ? {} : { agent_choice: agentChoice }),
          ...(skills.length > 0 ? { skills } : {}),
        };
        if (Object.keys(settings).length > 0) await updateProject(projectId, settings);
      }

      const desired = checked;
      // Attach newly-checked paths; a path owned elsewhere is skipped by the
      // disabled checkbox, but the daemon refuses it too (409 naming the owner).
      for (const path of desired) {
        if (!current.has(path)) await addProjectMember(projectId, path);
      }
      // Detach paths that were members but are now unchecked.
      for (const path of current) {
        if (!desired.has(path)) await removeProjectMember(projectId, path);
      }
      if (skillsChanged) announceSkillTiersChanged();
      onSaved();
      onClose();
    } catch (e) {
      // The Projet exists even though a later step failed: let the list show it.
      if (createdNow) onSaved();
      const msg =
        e instanceof ApiError
          ? e.message
          : e instanceof Error
            ? e.message
            : "failed to save project";
      setError(msg);
      setSubmitting(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="project-edit-backdrop"
      onClick={onClose}
    >
      <div
        className="w-[440px] max-w-[90vw] rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
        role="dialog"
        aria-label="Edit project"
        data-testid="project-edit-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-1 font-medium text-fg">
          {editing ? "Edit project" : "Name project"}
        </div>
        <p className="mb-3 text-fg-4" style={{ fontSize: "11px" }}>
          Group repositories that work together under one name. A repo belongs to
          at most one project; the harness set here applies to every Run whose
          primary repo is a member.
        </p>

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Name
        </label>
        <input
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setError(null);
          }}
          data-testid="project-name-input"
          className="mb-3 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg outline-none focus:border-acc"
          style={{ fontSize: "11px" }}
        />

        <div className="mb-3">
        <AgentControl
          choice={agentChoice}
          onChange={setAgentChoice}
          profiles={agentProfiles}
          catalog={harnessCatalog}
          inherited={{ harness: "claude", model: null, effort: null }}
          label="Agent — Project"
          testId="project-agent-control"
        />
        <div className="mt-2">
          {/* #669/ADR-0062: the Projet tier; the instance selection is inherited. */}
          <SkillSelector
            tier="project"
            own={skills}
            onChange={setSkills}
            inherited={[{ tier: "instance", skills: instanceSkills }]}
            bank={skillBank}
            label="Skills — Project"
            testId="project-skill-selector"
          />
        </div>
        <div className="sr-only" aria-hidden>
          <HarnessSelect
            value={harness}
            onChange={setHarness}
            catalog={harnessCatalog}
            inheritLabel="No harness (inherit)"
            data-testid="project-harness-select"
          />
        </div>

        {initialProject && (
          <div className="mb-3">
            {provisioningOpen ? (
              <PersistedProvisioningEditor
                scope="project"
                projectId={initialProject.id}
                initialRepository={initialProject.members[0] ?? ""}
              />
            ) : (
              <button
                type="button"
                onClick={() => setProvisioningOpen(true)}
                className="rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc"
              >
                Configure worktree provisioning…
              </button>
            )}
          </div>
        )}
        </div>

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Member repositories
        </label>
        <div
          className="mb-3 max-h-48 overflow-y-auto rounded border border-line-strong bg-bg-3"
          data-testid="project-members-list"
        >
          {candidates.length === 0 && (
            <div className="px-2 py-2 text-fg-4" style={{ fontSize: "11px" }}>
              No repositories to attach.
            </div>
          )}
          {candidates.map((path) => {
            const owner = ownerElsewhere.get(path);
            const disabled = owner != null;
            return (
              <label
                key={path}
                className={`flex items-center gap-2 border-b border-line-soft px-2 py-1.5 last:border-b-0 ${
                  disabled ? "opacity-50" : "cursor-pointer hover:bg-bg-4"
                }`}
                style={{ fontSize: "11px" }}
                title={owner ? `Already in project "${owner}"` : path}
                data-testid="project-member-row"
                data-path={path}
                data-disabled={disabled ? "true" : "false"}
              >
                <input
                  type="checkbox"
                  checked={checked.has(path)}
                  disabled={disabled}
                  onChange={() => toggle(path)}
                  data-testid="project-member-checkbox"
                />
                <span className="truncate text-fg">{path}</span>
                {owner && (
                  <span
                    className="ml-auto shrink-0 text-fg-4"
                    data-testid="project-member-owner"
                  >
                    in {owner}
                  </span>
                )}
              </label>
            );
          })}
        </div>

        {error && (
          <div
            className="mb-3 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-1.5 text-st-failed"
            style={{ fontSize: "11px" }}
            data-testid="project-edit-error"
          >
            {error}
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="cursor-pointer rounded border border-line-strong px-3 py-1 text-fg-3 transition-colors hover:bg-bg-3"
            style={{ fontSize: "11px" }}
            data-testid="project-edit-cancel"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={!canSave}
            className="cursor-pointer rounded bg-acc px-3 py-1 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:cursor-not-allowed disabled:opacity-50"
            style={{ fontSize: "11px" }}
            data-testid="project-edit-save"
          >
            {submitting ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
