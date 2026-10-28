import { useState } from "react";
import { useEditStore } from "../store/editStore";
import { useWiringStore } from "../../stores/wiringStore";
import { gridStep, resolveGridSize, isGridSize } from "../../lib/wiringGrid";
import type { VariableDef } from "../../shared/generated/VariableDef";
import type { VariableType } from "../../shared/generated/VariableType";
import type { EditorPipelineDef } from "../model/pipeline";
import { Plus, Variable, X } from "lucide-react";
import { EmptyState, Field, IconButton, KeyValue, Section, TextInput } from "../../shared/ui/kit";
import "./inspect.css";
import GridSizePicker from "./GridSizePicker";

const VAR_TYPES = ["int", "float", "string", "bool", "list"] as const;

/**
 * A pipeline's metadata — Identity, Variables, Canvas — as the Info tab of
 * Pipeline info shows it (Notion #6 / #948). Pipeline info is the ONLY surface of
 * these settings: the standalone Pipeline Inspector that used to appear on an
 * empty selection is gone.
 *
 * On a **template** every field is editable and goes through `updatePipelineMeta`
 * — the active tab's pipeline, which is the one the panel is shown for — so an
 * edit marks the tab dirty, is undoable, shows in the YAML tab and is written on
 * Save, like any other edit. On a **Run** (`readOnly`) the same values are shown
 * as text: no input is rendered at all.
 */
export default function PipelineMetaSections({
  pipeline,
  readOnly,
}: {
  pipeline: EditorPipelineDef;
  readOnly: boolean;
}) {
  const updateMeta = useEditStore((s) => s.updatePipelineMeta);
  const globalGridSize = useWiringStore((s) => s.defaultGridSize);

  const variables = Object.entries(pipeline.variables ?? {});
  const ownGridSize = isGridSize(pipeline.grid_size) ? pipeline.grid_size : null;
  const effectiveGridSize = resolveGridSize(pipeline.grid_size, globalGridSize);
  const promptRequired = pipeline.prompt_required !== false;
  const waitForStartedNodes = pipeline.wait_for_started_nodes === true;

  function handleAddVariable() {
    let name = "new_var";
    let counter = 1;
    while (pipeline.variables[name]) {
      name = `new_var_${++counter}`;
    }
    updateMeta({
      variables: {
        ...pipeline.variables,
        [name]: { type: "int", default: 0 },
      },
    });
  }

  // A rename rebuilds the map in its current order, the renamed entry in place:
  // the row keeps its position (and, keyed by index, its DOM input and focus)
  // while the user types the new name.
  function handleUpdateVariable(oldName: string, newName: string, updates: Partial<VariableDef>) {
    const current = pipeline.variables[oldName] ?? { type: "int", default: 0 };
    const newVars: Record<string, VariableDef> = {};
    for (const [k, v] of Object.entries(pipeline.variables)) {
      if (k === oldName) newVars[newName] = { ...current, ...updates };
      else newVars[k] = v;
    }
    if (!(oldName in pipeline.variables)) newVars[newName] = { ...current, ...updates };
    updateMeta({ variables: newVars });
  }

  function handleDeleteVariable(name: string) {
    const newVars = { ...pipeline.variables };
    delete newVars[name];
    updateMeta({ variables: newVars });
  }

  return (
    <div data-testid="pipeline-meta" data-readonly={readOnly ? "true" : "false"}>
      {/* Identity */}
      <div data-testid="pipeline-meta-identity">
        {/* Read only (a Run, a version): one Pipeline section, as the Run's. */}
        <Section title={readOnly ? "Pipeline" : "Identity"}>
          {readOnly ? (
            <KeyValue
              rows={[
                { k: "Name", v: <span data-testid="pipeline-meta-name">{pipeline.name}</span>, tip: pipeline.name },
                {
                  k: "Prompt",
                  v: <span data-testid="pipeline-meta-prompt-required">{promptRequired ? "Yes" : "No"}</span>,
                  tip: "Prompt required",
                },
                {
                  k: "Wait started",
                  v: <span data-testid="pipeline-meta-wait-for-started-nodes">{waitForStartedNodes ? "Yes" : "No"}</span>,
                  tip: "Wait for started nodes",
                },
                {
                  k: "Wiring grid",
                  v: <span data-testid="pipeline-meta-grid-size">{ownGridSize ?? `Global (${globalGridSize})`}</span>,
                },
                { k: "Nodes", v: <span data-testid="pipeline-graph-nodes">{pipeline.nodes.length}</span>, mono: true },
                { k: "Edges", v: <span data-testid="pipeline-graph-edges">{pipeline.edges.length}</span>, mono: true },
              ]}
            />
          ) : (
            <>
              <Field label="Name">
                <TextInput
                  value={pipeline.name}
                  onChange={(e) => updateMeta({ name: e.target.value })}
                  data-testid="pipeline-name-input"
                />
              </Field>
              {/* ADR-0088: no free « Version » field: a Save numbers the versions. */}
              {/* Prompt-required toggle (#158). Checked by default — unchecking lets a
                  Run launch without a prompt; the entry node sources its own work. */}
              <Field label="Prompt" help="Prompt required">
                <input
                  type="checkbox"
                  className="ei-checkbox"
                  checked={promptRequired}
                  onChange={(e) => updateMeta({ prompt_required: e.target.checked })}
                  data-testid="prompt-required-checkbox"
                />
              </Field>
              {/* ADR-0086 « Attente des nœuds lancés ». Off by default: a Run ends
                  at End. On, a Run that reaches End waits until no node it
                  launched is still running, waiting or awaiting you. */}
              <Field label="Wait started" help="Wait for started nodes">
                <input
                  type="checkbox"
                  className="ei-checkbox"
                  checked={waitForStartedNodes}
                  onChange={(e) => updateMeta({ wait_for_started_nodes: e.target.checked })}
                  data-testid="wait-for-started-nodes-checkbox"
                />
              </Field>
            </>
          )}
        </Section>
      </div>

      {/* Variables */}
      <div data-testid="info-panel-variables">
        <Section
          title="Variables"
          count={variables.length}
          flush
          aside={
            readOnly ? undefined : (
              <IconButton
                icon={Plus}
                size="sm"
                label="Add variable"
                onClick={handleAddVariable}
                data-testid="pipeline-variable-add"
              />
            )
          }
        >
          {variables.length === 0 && (
            <EmptyState icon={Variable} layout="row">
              None
            </EmptyState>
          )}
          {readOnly && variables.length > 0 && (
            <div className="ei-pad">
              <KeyValue
                rows={variables.map(([name, def]) => ({
                  k: name,
                  v: <span data-testid="pipeline-variable-value">{`${def.type} = ${formatVariableValue(def.default)}`}</span>,
                  mono: true,
                  tip: `${def.type} = ${formatVariableValue(def.default)}`,
                }))}
              />
            </div>
          )}
          {variables.map(([name, def], index) =>
            readOnly ? null : (
              <VariableRow
                key={index}
                name={name}
                isTaken={(candidate) => candidate !== name && candidate in pipeline.variables}
                def={def}
                onUpdate={(newName, updates) => handleUpdateVariable(name, newName, updates)}
                onDelete={() => handleDeleteVariable(name)}
              />
            ),
          )}
        </Section>
      </div>

      {/* Canvas (#877 / ADR-0076): the pipeline's own wiring-grid size. Saved
          in the file — a shared pipeline then draws on the same grid for
          everyone; « Global » stores nothing and follows the reader's default. */}
      {!readOnly && (
        <div data-testid="pipeline-meta-canvas">
          <Section title="Canvas">
            <Field label="Wiring grid" help="Existing routes keep their points">
              <span className="ei-line">
                <GridSizePicker
                  value={ownGridSize}
                  globalSize={globalGridSize}
                  onChange={(v) => updateMeta({ grid_size: v })}
                  ariaLabel="Wiring grid size"
                  testId="pipeline-grid-size"
                />
                <span className="ei-meta d-mono" title="Draws on" data-testid="pipeline-grid-size-hint">
                  {effectiveGridSize} · {gridStep(effectiveGridSize)}px
                </span>
              </span>
            </Field>
          </Section>
        </div>
      )}
    </div>
  );
}

function formatVariableValue(value: unknown): string {
  if (Array.isArray(value)) return `[${value.join(", ")}]`;
  if (typeof value === "string") return `"${value}"`;
  return String(value ?? "");
}

function VariableRow({
  name,
  isTaken,
  def,
  onUpdate,
  onDelete,
}: {
  name: string;
  /** Whether another variable of the pipeline already has this name. */
  isTaken: (candidate: string) => boolean;
  def: VariableDef;
  onUpdate: (newName: string, updates: Partial<VariableDef>) => void;
  onDelete: () => void;
}) {
  // The name being typed. It is committed on every keystroke while it is a valid
  // name; an empty or already-taken name stays local (flagged) so it never
  // overwrites another variable, and reverts on blur. Re-synced when the stored
  // name changes from elsewhere (undo, a deleted row above shifting this one).
  const [draftName, setDraftName] = useState(name);
  const [syncedName, setSyncedName] = useState(name);
  if (syncedName !== name) {
    setSyncedName(name);
    setDraftName(name);
  }
  const nameInvalid = draftName !== name;

  function handleNameChange(val: string) {
    setDraftName(val);
    if (val.trim() !== "" && !isTaken(val)) onUpdate(val, {});
  }

  const defaultStr = Array.isArray(def.default)
    ? `[${(def.default as unknown[]).join(", ")}]`
    : String(def.default ?? "");

  function handleDefaultChange(val: string) {
    let parsed: unknown = val;
    if (def.type === "int") parsed = parseInt(val, 10) || 0;
    else if (def.type === "float") parsed = parseFloat(val) || 0;
    else if (def.type === "bool") parsed = val === "true";
    else if (def.type === "list") {
      if (val.startsWith("[") && val.endsWith("]")) {
        parsed = val.slice(1, -1).split(",").map((s) => s.trim()).filter(Boolean);
      } else {
        parsed = val.split(",").map((s) => s.trim()).filter(Boolean);
      }
    }
    onUpdate(name, { default: parsed });
  }

  return (
    <div className="pi-var ei-pad" data-testid="pipeline-variable-row">
      <input
        className="d-input d-input--mono d-input--sm"
        value={draftName}
        onChange={(e) => handleNameChange(e.target.value)}
        onBlur={() => setDraftName(name)}
        aria-invalid={nameInvalid}
        aria-label="Variable name"
        title={nameInvalid ? (draftName.trim() === "" ? "A variable needs a name" : "Another variable already has this name") : name}
        data-testid="pipeline-variable-name"
      />
      <select
        value={def.type}
        onChange={(e) => onUpdate(name, { type: e.target.value as VariableType })}
        className="ei-select ei-select--sm ei-select--mono"
        aria-label="Variable type"
        data-testid="pipeline-variable-type"
      >
        {VAR_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
      </select>
      <TextInput
        mono
        small
        value={defaultStr}
        onChange={(e) => handleDefaultChange(e.target.value)}
        placeholder="default"
        aria-label="Variable default"
        data-testid="pipeline-variable-default"
      />
      <IconButton
        icon={X}
        size="sm"
        label={`Delete variable ${name}`}
        onClick={onDelete}
        data-testid="pipeline-variable-delete"
      />
    </div>
  );
}
