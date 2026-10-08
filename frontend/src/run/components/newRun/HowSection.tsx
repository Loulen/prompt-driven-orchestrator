import AgentControl from "../../../settings/components/AgentControl";
import HarnessSelect from "../../../harness/components/HarnessSelect";
import SkillSelector from "../../../skill/components/SkillSelector";
import PipelineSelect from "./PipelineSelect";
import SandboxField from "./SandboxField";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** ── HOW ── the pipeline, the sandbox, the agent and the skills. */
export default function HowSection({ form, onManageStagingProfiles }: { form: NewRunForm; onManageStagingProfiles?: () => void }) {
  const {
    pipelines, selectedPipeline, selectedPipelineId, repoValid, mode, harness, setHarness,
    agentChoice, setAgentChoice, skills, setSkills, agentProfiles, skillBank,
    inheritedSkillTiers, handlePipelineChange, harnessCatalog, instanceDefaultHarness,
    pipelineSections, repoProject,
  } = form;
  return (
    <div className="flex flex-col gap-3 py-4 border-b border-line">
      <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
        How
      </span>
      <div className="flex flex-col gap-1.5">
        <label
          htmlFor="pipeline-select"
          className="font-medium text-fg-2"
          style={{ fontSize: "11.5px" }}
        >
          Pipeline
        </label>
        {/* #822: the app's own menu, not a `<select>` — see `PipelineSelect`.
            Both dead ends (no repo yet, a repo with no pipelines) disable the
            trigger and say which one it is, where the `<select>` used to open
            onto a single greyed option that could not be chosen anyway. */}
        <div className="flex gap-1.5">
          <PipelineSelect
            pipelines={repoValid ? pipelines : []}
            sections={pipelineSections}
            project={repoProject}
            value={selectedPipelineId}
            onChange={handlePipelineChange}
            disabled={!repoValid || pipelines.length === 0}
            placeholder={
              !repoValid
                ? "Select a repository first"
                : pipelines.length === 0
                  ? "No pipelines found"
                  : "Choose a pipeline"
            }
          />
        </div>
        {selectedPipeline?.scope === "instance" && (
          <span className="inline-flex items-center gap-1 text-fg-4" style={{ fontSize: "10.5px" }}>
            {selectedPipeline.path}
          </span>
        )}
      </div>

      {/* Sandbox (#410/#432/#452): "Use instance default", `off`, or one of the
          instance's STAGING PROFILES — the options are data, served by `GET /settings`
          (names only). #452: BOTH modes lead with the inherit option, so "I am not
          choosing" is expressible in run mode too; it is the seeded value, and run mode
          names what it currently resolves to. Profiles are disabled when the daemon
          reports Docker unavailable (advisory greying — the run-advance fail-fast
          remains authoritative), and neither an unavailable Docker nor a vanished
          profile rewrites the field: both block the action and say so. */}
      <SandboxField form={form} onManageStagingProfiles={onManageStagingProfiles} />

      <div className="flex flex-col gap-1.5">
        <AgentControl
          choice={agentChoice}
          onChange={setAgentChoice}
          profiles={agentProfiles}
          catalog={harnessCatalog}
          inherited={{ harness: instanceDefaultHarness || "claude", model: null, effort: null }}
          label={mode === "trigger" ? "Agent — Trigger" : "Agent — New Run"}
          testId="run-agent-control"
        />
        <div className="sr-only" aria-hidden>
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
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {mode === "trigger"
            ? "Every fired Run launches on this harness. Nodes that pin their own harness ignore it."
            : "The harness every free node runs on for this Run. Nodes that pin their own harness ignore it."}
        </span>
      </div>

      {/* #669/ADR-0062: the Run tier of the skills selection. Inherited from
          the instance and the Projet owning the target repo; a Trigger stores
          the same list for every Run it fires. */}
      <div className="flex flex-col gap-1.5">
        <SkillSelector
          tier="run"
          own={skills}
          onChange={setSkills}
          inherited={inheritedSkillTiers}
          bank={skillBank}
          label={mode === "trigger" ? "Skills — Trigger" : "Skills — New Run"}
          testId="run-skill-selector"
        />
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {mode === "trigger"
            ? "Every fired Run adds these skills to every node, on top of the instance and project tiers."
            : "Added to every node of this Run, on top of the instance and project tiers. Nodes add their own."}
        </span>
      </div>
    </div>
  );
}
