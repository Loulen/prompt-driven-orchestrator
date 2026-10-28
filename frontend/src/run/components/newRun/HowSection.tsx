import AgentControl from "../../../settings/components/AgentControl";
import HarnessSelect from "../../../harness/components/HarnessSelect";
import SkillSelector from "../../../skill/components/SkillSelector";
import FormRow from "../../../shared/ui/FormRow";
import PipelineSelect from "./PipelineSelect";
import SandboxField from "./SandboxField";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** ── HOW ── the pipeline, the sandbox, the agent and the skills. */
export default function HowSection({ form, onManageStagingProfiles }: { form: NewRunForm; onManageStagingProfiles?: () => void }) {
  const {
    pipelines, selectedPipelineId, repoValid, mode, harness, setHarness,
    agentChoice, setAgentChoice, skills, setSkills, agentProfiles, skillBank,
    inheritedSkillTiers, handlePipelineChange, harnessCatalog, instanceDefaultHarness,
    pipelineSections, repoProject, promptAddedSkills, remoteUrl,
  } = form;
  return (
    <>
      <FormRow label="Pipeline" htmlFor="pipeline-select">
        {/* #822: the app's own menu, not a `<select>` — see `PipelineSelect`.
            Both dead ends (no repo yet, a repo with no pipelines) disable the
            trigger and say which one it is. */}
        <PipelineSelect
          pipelines={repoValid ? pipelines : []}
          sections={pipelineSections}
          project={repoProject}
          value={selectedPipelineId}
          onChange={handlePipelineChange}
          disabled={!repoValid || pipelines.length === 0}
          placeholder={
            remoteUrl
              ? "After the clone"
              : !repoValid
                ? "Choose a repository first"
                : pipelines.length === 0
                  ? "No pipeline"
                  : "Choose a pipeline"
          }
        />
      </FormRow>

      {/* Sandbox (#410/#432/#452): the instance default, `off`, or one of the
          instance's staging profiles. */}
      <SandboxField form={form} onManageStagingProfiles={onManageStagingProfiles} />

      <FormRow label="Agent" title={mode === "trigger" ? "Every fired Run, free nodes only" : "Every free node of the Run"}>
        <AgentControl
          choice={agentChoice}
          onChange={setAgentChoice}
          profiles={agentProfiles}
          catalog={harnessCatalog}
          inherited={{ harness: instanceDefaultHarness || "claude", model: null, effort: null }}
          label={mode === "trigger" ? "Agent — Trigger" : "Agent — New Run"}
          showLabel={false}
          testId="run-agent-control"
        />
        <div className="d-sr" aria-hidden>
          <HarnessSelect
            id="harness-select"
            data-testid="harness-select"
            value={harness}
            onChange={setHarness}
            catalog={harnessCatalog}
            inheritLabel="Use instance default"
            inheritHint={mode === "run" ? (instanceDefaultHarness ?? undefined) : undefined}
          />
        </div>
      </FormRow>

      {/* #669/ADR-0062: the Run tier of the skills selection, on top of the
          instance's and the Projet's; a Trigger stores it for every fired Run. */}
      <FormRow label="Skills" title="Added to every node, on top of the instance and project tiers">
        <SkillSelector
          tier="run"
          own={skills}
          onChange={setSkills}
          inherited={inheritedSkillTiers}
          bank={skillBank}
          label={mode === "trigger" ? "Skills — Trigger" : "Skills — New Run"}
          showLabel={false}
          testId="run-skill-selector"
          added={promptAddedSkills}
        />
      </FormRow>
    </>
  );
}
