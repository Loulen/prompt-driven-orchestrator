import { useMemo, useState } from "react";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { Project } from "../model/project";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { addProjectMember, createProject, removeProjectMember, updateProject } from "../api/projects";
import { ApiError } from "../../shared/http/client";
import { useHarnessCatalog } from "../../harness/store/useHarnessCatalog";
import { useAgentProfiles } from "../../settings/store/useAgentProfiles";
import AgentControl from "../../settings/components/AgentControl";
import HarnessSelect from "../../harness/components/HarnessSelect";
import PersistedProvisioningEditor from "./PersistedProvisioningEditor";
import SkillSelector from "../../skill/components/SkillSelector";
import { useSkillBank } from "../../skill/store/useSkillBank";
import { announceSkillTiersChanged, useSkillTiers } from "../../skill/store/useSkillTiers";
import ExtensionSections from "../../shared/extensions/ExtensionSections";
import ProjectMembersList from "./ProjectMembersList";
import { FolderGit2 } from "lucide-react";
import { Button, Kbd, Modal, TextInput } from "../../shared/ui/kit";
import FormRow, { FormSection } from "../../shared/ui/FormRow";
import type { ProjectEditContext } from "../../shared/extensions/contract";
import { sectionSaveError, useSectionForm } from "../../shared/extensions/sectionForm";

/**
 * The group-header pencil (#552, ADR-0046): name a Projet (or rename an existing
 * one), pick which repo paths are its members, and optionally pose the harness it
 * carries. Naming a group is what **materialises** the Projet — nothing is seeded
 * until this saves (ADR-0046).
 *
 * A repository may be a member of several Projets (ADR-0087): a candidate held by
 * another Projet names it, and stays free to check. The default primary repository
 * — the one New Run preselects for this Projet — is chosen among the members.
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
  home,
}: {
  /** The Projet being edited, or `null` when the pencil creates a fresh one. */
  initialProject: Project | null;
  /** Pre-filled name: the Projet's name, or the group's derived label. */
  initialName: string;
  /** Pre-checked member paths: the Projet's members, or the group's own path. */
  initialMemberPaths: string[];
  /** Candidate repo paths to attach (the surrounding list's distinct repos). */
  availableRepos: string[];
  /** All Projets, to name the other Projets holding a candidate. */
  projects: Project[];
  onClose: () => void;
  /** The Projet saved (created or edited); called again when a retry saves it. */
  onSaved: (project?: Project) => void;
  /** The daemon's home, to read a path as `~/code/…`. */
  home?: string | null;
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
  const [primary, setPrimary] = useState<string | null>(initialProject?.primary_repo ?? null);
  // The default primary is a member: the chosen one while checked, else the
  // first checked candidate (the daemon's rule for a Projet that chose none).
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // #586: the harness options, dynamic from `/settings` (floor ∪ descriptors,
  // each installed/not). The modal holds no settings of its own, so it fetches.
  const harnessCatalog = useHarnessCatalog();
  const { profiles: agentProfiles } = useAgentProfiles();
  // ADR-0081 (#31): the extension sections of `project/edit`. They keep a draft
  // and apply it once the core saved (a fresh Projet has its id only then);
  // a failing section keeps the modal open, the Projet itself already saved.
  const sections = useSectionForm();
  const sectionContext = useMemo<Omit<ProjectEditContext, "location" | "run" | "project">>(
    () => ({
      entity: {
        kind: "project",
        id: editing?.id ?? null,
        name: name.trim(),
        attributes: editing?.attributes ?? {},
      },
      creating: editing == null,
    }),
    [editing, name],
  );
  function cancel() {
    sections.cancel();
    onClose();
  }

  // path → the names of the other Projets holding it (ADR-0087: a repository
  // may be in several), excluding the one being edited.
  const alsoIn = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const p of projects) {
      if (editing && p.id === editing.id) continue;
      for (const path of p.members) map.set(path, [...(map.get(path) ?? []), p.name]);
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

  const effectivePrimary =
    primary !== null && checked.has(primary)
      ? primary
      : (candidates.find((path) => checked.has(path)) ?? null);

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
      let attributes = editing?.attributes ?? {};
      let savedProject: Project | undefined = editing ?? undefined;
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
        savedProject = project;
        projectId = project.id;
        current = new Set(project.members);
        attributes = project.attributes ?? {};
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
      // Attach newly-checked paths (a path of another Projet joins this one too).
      for (const path of desired) {
        if (!current.has(path)) await addProjectMember(projectId, path);
      }
      // Detach paths that were members but are now unchecked.
      for (const path of current) {
        if (!desired.has(path)) await removeProjectMember(projectId, path);
      }
      // The default primary, once its repository is a member (ADR-0087).
      if (primary !== null && effectivePrimary !== (editing?.primary_repo ?? null)) {
        await updateProject(projectId, { primary_repo: effectivePrimary });
      }
      if (skillsChanged) announceSkillTiersChanged();
      try {
        await sections.save({
          kind: "project",
          id: projectId,
          name: trimmedName,
          attributes,
        });
      } catch (cause) {
        // The Projet is saved: the list shows it; the section may retry.
        onSaved(savedProject);
        setError(sectionSaveError(cause));
        setSubmitting(false);
        return;
      }
      onSaved(savedProject);
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

  const memberCount = checked.size;
  return (
    <Modal
      size="l"
      icon={<FolderGit2 size={14} strokeWidth={1.5} className="ln-faint" aria-hidden />}
      title={
        editing ? (
          <>
            Edit project <span className="ln-pmodal__name">{editing.name}</span>
          </>
        ) : (
          "New project"
        )
      }
      titleText={editing ? `Edit project ${editing.name}` : "New project"}
      onClose={cancel}
      onSubmit={() => void handleSave()}
      footerNote={
        <span className="ln-mono" data-testid="project-edit-summary">
          {memberCount} repo{memberCount === 1 ? "" : "s"}
        </span>
      }
      actions={
        <>
          <Button onClick={cancel} data-testid="project-edit-cancel">
            Cancel
          </Button>
          <Button tone="primary" onClick={() => void handleSave()} disabled={!canSave} data-testid="project-edit-save">
            {submitting ? "Saving…" : "Save"}
            {!submitting && <Kbd>Ctrl ↵</Kbd>}
          </Button>
        </>
      }
      data-testid="project-edit-modal"
    >
      {/* #940: bounded to the window like New Run — only the body scrolls. */}
      <div className="ln-form" data-testid="project-edit-body">
        <div className="ln-sec">
          <FormRow label="Name" htmlFor="project-name">
            <TextInput
              id="project-name"
              className="ln-grow"
              value={name}
              placeholder="Project name"
              onChange={(e) => {
                setName(e.target.value);
                setError(null);
              }}
              data-autofocus=""
              data-testid="project-name-input"
            />
          </FormRow>
          <FormRow label="Agent" title="Every Run launched in this project">
            <AgentControl
              choice={agentChoice}
              onChange={setAgentChoice}
              profiles={agentProfiles}
              catalog={harnessCatalog}
              inherited={{ harness: "claude", model: null, effort: null }}
              label="Agent — Project"
              showLabel={false}
              testId="project-agent-control"
            />
            <div className="d-sr" aria-hidden>
              <HarnessSelect
                value={harness}
                onChange={setHarness}
                catalog={harnessCatalog}
                inheritLabel="No harness (inherit)"
                data-testid="project-harness-select"
              />
            </div>
          </FormRow>
          <FormRow label="Skills" title="The project tier, on top of the instance's">
            {/* #669/ADR-0062: the Projet tier; the instance selection is inherited. */}
            <SkillSelector
              tier="project"
              own={skills}
              onChange={setSkills}
              inherited={[{ tier: "instance", skills: instanceSkills }]}
              bank={skillBank}
              label="Skills — Project"
              showLabel={false}
              testId="project-skill-selector"
            />
          </FormRow>
          {initialProject && (
            <PersistedProvisioningEditor
              scope="project"
              projectId={initialProject.id}
              initialRepository={initialProject.members[0] ?? ""}
            />
          )}
        </div>

        <FormSection label="Repositories">
          <ProjectMembersList
            candidates={candidates}
            checked={checked}
            alsoIn={alsoIn}
            primary={effectivePrimary}
            onToggle={toggle}
            onPrimary={setPrimary}
            home={home}
          />
        </FormSection>

        {/* ADR-0081 (#31): the `project/edit` sections, under the core's fields,
            one per contribution in registration order. */}
        <div className="ln-ext empty:hidden">
          <ExtensionSections<ProjectEditContext>
            location="project/edit"
            extra={sectionContext}
            form={sections.form}
          />
        </div>

        {error && (
          <div className="ln-alert" role="alert" data-testid="project-edit-error">
            {error}
          </div>
        )}
      </div>
    </Modal>
  );
}
