/**
 * Settings › Agents: the instance tier of the agent choice and the default harness and
 * models (instance form), the agentic profiles (their own persistence), the manager's
 * auto-start and pin, the instance tier of the skills selection and the way into the bank.
 */

import { FileText } from "lucide-react";
import AgentControl from "../AgentControl";
import HarnessSelect from "../../../harness/components/HarnessSelect";
import ModelPicker from "../../../harness/components/ModelPicker";
import { findHarnessOption, type HarnessCatalog } from "../../../harness/model/catalog";
import type { AgentProfile } from "../../../shared/generated/AgentProfile";
import SkillSelector from "../../../skill/components/SkillSelector";
import type { SkillBank } from "../../../skill/model/skill";
import type { SettingsSection } from "../../model/settingsCategories";
import {
  managerEnabledSourceNote,
  managerProfileSourceNote,
  modelSourceNote,
} from "../../model/sourceNotes";
import AgentProfilesPanel from "../AgentProfilesPanel";
import { CheckboxRow, FieldBlock } from "./fields";
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

  return (
    <>
    <Section section={sections[0]}>
      <AgentControl
        choice={values.agentChoice}
        onChange={(choice) => setField("agentChoice", choice)}
        profiles={agentProfiles}
        catalog={catalog}
        inherited={{ harness: "claude", model: null, effort: null }}
        allowInherit={false}
        label="Agent — Instance settings"
        testId="instance-agent-control"
      />
      {/* Default harness (#550/ADR-0046). Precedence at spawn:
          node → Run → Projet → instance (this) → claude floor. */}
      <FieldBlock
        label="Default harness"
        testId="setting-default-harness"
        dirty={dirty.has("default-harness")}
        help={
          <>
            Every new node runs on this harness unless it pins its own. "Default"
            leaves it to the <span className="font-mono">claude</span> floor.
          </>
        }
        source={modelSourceNote(settings.default_harness)}
        sourceTestId="setting-source-default-harness"
      >
        <HarnessSelect
          value={values.defaultHarness}
          onChange={(v) => setField("defaultHarness", v)}
          catalog={catalog}
          inheritLabel="Default (claude floor)"
          data-testid="setting-default-harness-select"
          className={`w-full rounded border bg-surface-2 px-2 py-1 text-text ${
            dirty.has("default-harness") ? "border-st-await" : "border-line-strong"
          }`}
          style={{ fontSize: "11px" }}
        />
      </FieldBlock>
      {/* Default model (#347): folds under `claude` at resolve (#616). */}
      <FieldBlock
        label="Default model"
        dirty={dirty.has("default-model")}
        help={
          <>
            The model every work node launches with unless it sets its own.
            "Default" leaves it to your Claude account (no{" "}
            <span className="font-mono">--model</span>).
          </>
        }
        source={modelSourceNote(settings.default_model)}
        sourceTestId="setting-source-default-model"
      >
        <ModelPicker
          value={values.model}
          onChange={(v) => setField("model", v)}
          models={findHarnessOption(catalog, "claude")?.models ?? []}
          testid="default-model"
          subject="instance-default"
        />
      </FieldBlock>
      {/* Default model per harness (#550, #616 correctif 1): rows DERIVED from
          the served harness list; saving sends the map whole. */}
      <FieldBlock
        label="Default model per harness"
        testId="setting-default-harness-model"
        dirty={dirty.has("harness-models")}
        help="The model a node runs with on that harness when it sets none. Empty = the harness account default."
      >
        {[...catalog.builtin, ...catalog.descriptors].map((h) => (
          <label
            key={h.name}
            className="flex items-center gap-2 text-text-3"
            style={{ fontSize: "11px" }}
          >
            <span className="w-16 font-mono">{h.name}</span>
            <input
              type="text"
              value={values.harnessModels[h.name] ?? ""}
              onChange={(e) =>
                setField("harnessModels", {
                  ...values.harnessModels,
                  [h.name]: e.target.value,
                })
              }
              placeholder="account default"
              data-testid={`setting-default-model-${h.name}`}
              className={`flex-1 rounded border bg-surface-2 px-2 py-1 text-text ${
                dirty.has("harness-models")
                  ? "border-st-await"
                  : "border-line-strong"
              }`}
            />
          </label>
        ))}
      </FieldBlock>
    </Section>
    <Section section={sections[1]}>
      {/* Inline, list-first (#691): its own REST resource, its own buttons. */}
      <AgentProfilesPanel
        key={agentProfiles[0]?.id ?? "loading"}
        profiles={agentProfiles}
        onChanged={onProfilesChanged}
      />
    </Section>
    <Section section={sections[2]}>
      {/* Manager on demand: the auto-start flag (off by default — a
          Run starts managerless) and the profile pin, both part of
          the instance form (Save). A manual start from the Manager
          tab is ALWAYS available — the flag is a default, not a
          permission. */}
      <CheckboxRow
        id="manager-enabled"
        checked={values.managerEnabled}
        onChange={(v) => setField("managerEnabled", v)}
        dirty={dirty.has("manager-enabled")}
        label="Start the manager with each Run"
        help={
          <>
            When on, every Run launch spawns its{" "}
            <span className="font-mono">pdo-mgr-…</span> session — the
            pre-change behaviour. When off (the new default), the Run
            starts managerless and you start one from the Manager tab
            when you want it. This is only the <strong>default</strong>: a
            manual start is never blocked.
          </>
        }
        source={managerEnabledSourceNote(settings.manager_enabled)}
      />
      {/* The pin. Stored as a profile NAME: a stored name whose
          profile was later deleted (or renamed) gets the #432
          tombstone treatment — a « missing » option and a warning,
          never a silent rewrite. The spawn itself falls back to
          « Follow the Run ». */}
      <FieldBlock
        label="Manager profile"
        htmlFor="setting-manager-profile"
        dirty={dirty.has("manager-profile")}
        help={
          <>
            Which agent preset the manager runs as. « Follow the Run »
            keeps today's rule — the manager mirrors the Run's harness
            and lands on the default profile for model and effort. A
            named profile <strong>pins</strong> the manager to that
            harness · model · effort, whatever the Run chose (e.g. a
            cheaper model, since a manager mostly talks). Picks from
            Settings › Agents › Agent profiles.
          </>
        }
        source={managerProfileSourceNote(settings.manager_profile)}
        sourceTestId="setting-source-manager-profile"
      >
        <select
          id="setting-manager-profile"
          data-testid="setting-manager-profile"
          value={values.managerProfile}
          onChange={(e) => setField("managerProfile", e.target.value)}
          className={`w-[280px] rounded-md border bg-surface-2 px-2.5 py-1.5 text-text transition-colors focus:border-primary focus:outline-none ${
            dirty.has("manager-profile") ? "border-st-await" : "border-line-strong"
          }`}
          style={{ fontSize: "12px" }}
        >
          {/* "" = unset = « Follow the Run » — today's rule, unchanged. */}
          <option value="">Follow the Run</option>
          {agentProfiles.map((profile) => (
            <option key={profile.id} value={profile.name}>
              {profile.name}
            </option>
          ))}
          {/* Tombstone (#432): a stored name is NEVER silently rewritten. */}
          {missingManagerProfile && (
            <option value={values.managerProfile} data-testid="setting-manager-profile-missing">
              {values.managerProfile} — missing
            </option>
          )}
        </select>
        {missingManagerProfile && (
          <div
            className="text-st-await"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-manager-profile-missing-note"
          >
            No agent profile named « {values.managerProfile} » any more. The manager
            falls back to « Follow the Run » until you pick another one (or clear
            this).
          </div>
        )}
      </FieldBlock>
    </Section>
    <Section section={sections[3]}>
      {/* #669/ADR-0062: the instance tier of the skills selection — part of the
          instance form (Save), unlike the bank below it. */}
      <SkillSelector
        tier="instance"
        own={values.instanceSkills}
        onChange={(skills) => setField("instanceSkills", skills)}
        bank={skillBank}
        label="Skills — Instance settings"
        testId="instance-skill-selector"
      />
      {/* The bank is a journey of its own (browse, read, import, update from
          source): it opens as its own surface, not inline. */}
      <div
        className="flex items-center justify-between gap-3 rounded-md border border-line bg-surface-2/40 px-3 py-2.5"
        data-testid="setting-skill-bank-card"
      >
        <div className="flex flex-col gap-0.5">
          <span className="font-medium text-text-2" style={{ fontSize: "11.5px" }}>
            Skill bank
          </span>
          <span className="text-text-3" style={{ fontSize: "10.5px" }} data-testid="setting-skills-count">
            {skillsLoaded
              ? `${skillBank.skills.length} skill${skillBank.skills.length === 1 ? "" : "s"} · ${skillBank.folders.length} folder${skillBank.folders.length === 1 ? "" : "s"} · ~/.pdo/skills`
              : ""}
          </span>
        </div>
        <button
          type="button"
          data-testid="setting-open-skill-bank"
          onClick={onOpenSkillBank}
          className="flex items-center gap-1.5 rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5 text-text-2 transition-colors hover:border-primary"
          style={{ fontSize: 11 }}
        >
          <FileText size={11} />
          Open skill bank
        </button>
      </div>
    </Section>
    </>
  );
}
