/**
 * Settings › Agents: the instance tier of the agent choice and the default harness and
 * models (instance form), the agentic profiles (their own persistence), the manager's
 * auto-start and pin, the instance tier of the skills selection and the way into the bank.
 */

import { Library } from "lucide-react";
import AgentControl from "../AgentControl";
import { inheritedFloor } from "../../model/agentProfiles";
import HarnessSelect from "../../../harness/components/HarnessSelect";
import ModelPicker from "../../../harness/components/ModelPicker";
import { findHarnessOption, type HarnessCatalog } from "../../../harness/model/catalog";
import type { AgentProfile } from "../../../shared/generated/AgentProfile";
import { Button, TextInput } from "../../../shared/ui/kit";
import SkillSelector from "../../../skill/components/SkillSelector";
import type { SkillBank } from "../../../skill/model/skill";
import type { SettingsSection } from "../../model/settingsCategories";
import {
  harnessSourceNote,
  managerEnabledSourceNote,
  managerProfileSourceNote,
  modelSourceNote,
} from "../../model/sourceNotes";
import AgentProfilesPanel from "../AgentProfilesPanel";
import { SettingRow, SwitchRow } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import Section from "./Section";

export default function AgentsSections({
  sections,
  values,
  settings,
  dirty,
  setField,
  catalog,
  agentProfiles,
  onProfilesChanged,
  skillBank,
  skillsLoaded,
  onOpenSkillBank,
}: Omit<FormSectionProps, "section"> & {
  sections: SettingsSection[];
  catalog: HarnessCatalog;
  agentProfiles: AgentProfile[];
  onProfilesChanged: () => Promise<void>;
  skillBank: SkillBank;
  skillsLoaded: boolean;
  onOpenSkillBank: () => void;
}) {
  // Manager on demand (#432 tombstone): a stored pin whose profile no longer
  // exists (deleted or renamed) is never silently rewritten — the select keeps
  // the stored value under a « missing » option and a warning names the
  // fallback the spawn will take.
  const missingManagerProfile =
    values.managerProfile !== "" &&
    !agentProfiles.some((profile) => profile.name === values.managerProfile);
  const harnesses = [...catalog.builtin, ...catalog.descriptors];

  return (
    <>
      <Section section={sections[0]}>
        <SettingRow
          label="Agent"
          help="The instance tier: a profile, a custom combination; a node, a Run or a Project choose before it"
          dirty={dirty.has("agent-choice")}
          testId="setting-row-agent-choice"
        >
          <span className="set-ctl-w">
            <AgentControl
              choice={values.agentChoice}
              onChange={(choice) => setField("agentChoice", choice)}
              profiles={agentProfiles}
              catalog={catalog}
              inherited={inheritedFloor(agentProfiles)}
              allowInherit={false}
              label="Agent — Instance settings"
              showLabel={false}
              testId="instance-agent-control"
            />
          </span>
        </SettingRow>
        {/* Default harness (#550/ADR-0046). Precedence at spawn:
            node → Run → Projet → instance (this) → claude floor. */}
        <SettingRow
          label="Default harness"
          help="Every new node runs on it unless it pins its own; Default leaves it to the claude floor"
          source={harnessSourceNote(settings.default_harness)}
          keywords="PDO_DEFAULT_HARNESS"
          dirty={dirty.has("default-harness")}
          testId="setting-row-default-harness"
        >
          <HarnessSelect
            value={values.defaultHarness}
            onChange={(v) => setField("defaultHarness", v)}
            catalog={catalog}
            inheritLabel="Default (claude floor)"
            data-testid="setting-default-harness-select"
            className="d-input set-ctl-w"
          />
        </SettingRow>
        {/* Default model (#347): folds under `claude` at resolve (#616). */}
        <SettingRow
          label="Default model"
          help="The model a work node launches with unless it sets its own; Default leaves it to the account"
          source={modelSourceNote(settings.default_model)}
          keywords="PDO_DEFAULT_MODEL"
          dirty={dirty.has("default-model")}
          testId="setting-row-default-model"
        >
          <span className="set-ctl-w">
            <ModelPicker
              value={values.model}
              onChange={(v) => setField("model", v)}
              models={findHarnessOption(catalog, "claude")?.models ?? []}
              testid="default-model"
              subject="instance-default"
            />
          </span>
        </SettingRow>
        {/* Default model per harness (#550, #616 correctif 1): rows DERIVED from the
            served harness list; saving sends the map whole. */}
        {harnesses.map((h) => (
          <SettingRow
            key={h.name}
            label={`Model · ${h.name}`}
            help="The model a node runs with on this harness when it sets none; empty, the account default"
            keywords="default model per harness"
            dirty={dirty.has("harness-models") && (values.harnessModels[h.name] ?? "").trim() !== (settings.default_harness_model.stored[h.name] ?? "")}
            htmlFor={`setting-default-model-${h.name}`}
            testId={`setting-default-harness-model-${h.name}`}
          >
            <TextInput
              id={`setting-default-model-${h.name}`}
              data-testid={`setting-default-model-${h.name}`}
              mono
              className="set-ctl-w"
              placeholder="account default"
              value={values.harnessModels[h.name] ?? ""}
              onChange={(e) => setField("harnessModels", { ...values.harnessModels, [h.name]: e.target.value })}
            />
          </SettingRow>
        ))}
      </Section>

      {/* Inline, list-first (#691): its own REST resource, its own buttons. */}
      <AgentProfilesPanel
        key={agentProfiles[0]?.id ?? "loading"}
        section={sections[1]}
        profiles={agentProfiles}
        onChanged={onProfilesChanged}
      />

      <Section section={sections[2]}>
        {/* Manager on demand: the auto-start flag (off by default — a Run starts
            managerless) and the profile pin, both part of the instance form (Save). A
            manual start from the Manager tab is ALWAYS available. */}
        <SwitchRow
          label="Start with each run"
          help="Off, a Run starts without its manager; start one from the Manager tab"
          source={managerEnabledSourceNote(settings.manager_enabled)}
          testId="setting-row-manager-enabled"
          checked={values.managerEnabled}
          onChange={(v) => setField("managerEnabled", v)}
          dirty={dirty.has("manager-enabled")}
        />
        {/* The pin, stored as a profile NAME: a stored name whose profile was later
            deleted (or renamed) gets the #432 tombstone treatment — a « missing »
            option and a warning, never a silent rewrite. */}
        <SettingRow
          label="Manager profile"
          help="Follow the run mirrors the Run's harness; a profile pins the manager to its harness, model and effort"
          source={managerProfileSourceNote(settings.manager_profile)}
          dirty={dirty.has("manager-profile")}
          htmlFor="setting-manager-profile"
          testId="setting-row-manager-profile"
          note={missingManagerProfile ? `« ${values.managerProfile} » is gone: follows the run` : undefined}
          noteTone="warn"
          noteTestId="setting-manager-profile-missing-note"
        >
          <select
            id="setting-manager-profile"
            data-testid="setting-manager-profile"
            value={values.managerProfile}
            onChange={(e) => setField("managerProfile", e.target.value)}
            className="d-input set-ctl-w"
          >
            {/* "" = unset = « Follow the run » — today's rule, unchanged. */}
            <option value="">Follow the run</option>
            {agentProfiles.map((profile) => (
              <option key={profile.id} value={profile.name}>
                {profile.name}
              </option>
            ))}
            {/* Tombstone (#432): a stored name is NEVER silently rewritten. */}
            {missingManagerProfile && (
              <option value={values.managerProfile} data-testid="setting-manager-profile-missing">
                {values.managerProfile} (missing)
              </option>
            )}
          </select>
        </SettingRow>
      </Section>

      <Section
        section={sections[3]}
        tools={
          <>
            <span className="set-faint text-xs" data-testid="setting-skills-count">
              {skillsLoaded
                ? `${skillBank.skills.length} skill${skillBank.skills.length === 1 ? "" : "s"} · ${skillBank.folders.length} folder${skillBank.folders.length === 1 ? "" : "s"}`
                : ""}
            </span>
            <Button size="sm" icon={Library} onClick={onOpenSkillBank} data-testid="setting-open-skill-bank">
              Skill bank
            </Button>
          </>
        }
      >
        {/* #669/ADR-0062: the instance tier of the skills selection — part of the
            instance form (Save), unlike the bank it comes from. */}
        <SettingRow
          label="Instance skills"
          help="Every node of every Run receives them"
          kind="wide"
          dirty={dirty.has("skills")}
          testId="setting-row-instance-skills"
        >
          <SkillSelector
            tier="instance"
            own={values.instanceSkills}
            onChange={(skills) => setField("instanceSkills", skills)}
            bank={skillBank}
            label="Skills — Instance settings"
            testId="instance-skill-selector"
          />
        </SettingRow>
      </Section>
    </>
  );
}
