import { useState } from "react";
import { useEditStore } from "../store/editStore";
import { useWiringStore } from "../../stores/wiringStore";
import { gridStep, resolveGridSize, isGridSize } from "../../lib/wiringGrid";
import type { VariableDef } from "../../shared/generated/VariableDef";
import type { VariableType } from "../../shared/generated/VariableType";
import type { EditorPipelineDef } from "../model/pipeline";
import { SectionHead, Field } from "./InspectorPrimitives";
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
    <div
      className="flex flex-col gap-3 border-b border-line px-3 py-3"
      style={{ fontSize: "11.5px" }}
      data-testid="pipeline-meta"
      data-readonly={readOnly ? "true" : "false"}
    >
      {/* Identity */}
      <div className="flex flex-col gap-2" data-testid="pipeline-meta-identity">
        <SectionHead title="Identity" />
        {readOnly ? (
          <>
            <ReadOnlyRow label="Name" testid="pipeline-meta-name">{pipeline.name}</ReadOnlyRow>
            <ReadOnlyRow label="Prompt required" testid="pipeline-meta-prompt-required">
              {promptRequired ? "Yes" : "No"}
            </ReadOnlyRow>
            <ReadOnlyRow label="Wait for started nodes" testid="pipeline-meta-wait-for-started-nodes">
              {waitForStartedNodes ? "Yes" : "No"}
            </ReadOnlyRow>
          </>
        ) : (
          <>
            <Field label="Name">
              <input
                value={pipeline.name}
                onChange={(e) => updateMeta({ name: e.target.value })}
                className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
                data-testid="pipeline-name-input"
              />
            </Field>
            {/* ADR-0088: no free « Version » field: a Save numbers the versions. */}
            {/* Prompt-required toggle (#158). Checked by default — unchecking lets a
                Run launch without a prompt; the entry node sources its own work. */}
            <label className="flex items-center gap-2 text-fg-2" style={{ fontSize: "11px" }}>
              <input
                type="checkbox"
                className="accent-acc"
                checked={promptRequired}
                onChange={(e) => updateMeta({ prompt_required: e.target.checked })}
                data-testid="prompt-required-checkbox"
              />
              Prompt required
            </label>
            {/* ADR-0086 « Attente des nœuds lancés ». Off by default: a Run ends
                at End. On, a Run that reaches End waits until no node it
                launched is still running, waiting or awaiting you. */}
            <label
              className="flex items-center gap-2 text-fg-2"
              style={{ fontSize: "11px" }}
              title="A Run that reaches End keeps running until every node it launched is done (none running, waiting or awaiting you). An on-demand node never launched holds nothing."
            >
              <input
                type="checkbox"
                className="accent-acc"
                checked={waitForStartedNodes}
                onChange={(e) => updateMeta({ wait_for_started_nodes: e.target.checked })}
                data-testid="wait-for-started-nodes-checkbox"
              />
              Wait for started nodes
            </label>
          </>
        )}
      </div>

      {/* Variables */}
      <div className="flex flex-col gap-1" data-testid="info-panel-variables">
        <SectionHead
          title="Variables"
          count={variables.length}
          onAdd={readOnly ? undefined : handleAddVariable}
          addTestId="pipeline-variable-add"
        />
        {variables.length === 0 && (
          <div className="text-fg-4" style={{ fontSize: "10px" }}>
            No variables.
          </div>
        )}
        {variables.map(([name, def], index) =>
          readOnly ? (
            <div
              key={name}
              className="flex items-center justify-between gap-2 rounded bg-bg-3 px-2 py-1"
              style={{ fontSize: "10.5px" }}
              data-testid="pipeline-variable-row"
            >
              <span className="font-mono text-fg-3">{name}</span>
              <span className="flex items-center gap-2 font-mono text-fg-4">
                <span className="text-fg-5">{def.type}</span>
                {formatVariableValue(def.default)}
              </span>
            </div>
          ) : (
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
      </div>

      {/* Canvas (#877 / ADR-0076): the pipeline's own wiring-grid size. Saved
          in the file — a shared pipeline then draws on the same grid for
          everyone; « Global » stores nothing and follows the reader's default. */}
      <div className="flex flex-col gap-2" data-testid="pipeline-meta-canvas">
        <SectionHead title="Canvas" />
        {readOnly ? (
          <ReadOnlyRow label="Wiring grid" testid="pipeline-meta-grid-size">
            {ownGridSize ?? `Global (${globalGridSize})`}
          </ReadOnlyRow>
        ) : (
          <>
            <Field label="Wiring grid">
              <GridSizePicker
                value={ownGridSize}
                globalSize={globalGridSize}
                onChange={(v) => updateMeta({ grid_size: v })}
                ariaLabel="Wiring grid size"
                testId="pipeline-grid-size"
              />
            </Field>
            <div className="text-fg-4" style={{ fontSize: "10px" }} data-testid="pipeline-grid-size-hint">
              Draws on {effectiveGridSize} · {gridStep(effectiveGridSize)}px. Existing routes keep
              their points.
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function ReadOnlyRow({
  label,
  children,
  testid,
}: {
  label: string;
  children: React.ReactNode;
  testid?: string;
}) {
  return (
    <div
      className="flex items-center justify-between rounded bg-bg-3 px-2 py-1"
      style={{ fontSize: "10.5px" }}
      data-testid={testid}
    >
      <span className="text-fg-3">{label}</span>
      <span className="font-mono text-fg-4">{children}</span>
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
    <div
      className="flex items-center gap-1 rounded border border-line-soft bg-bg-3 px-2 py-1"
      data-testid="pipeline-variable-row"
    >
      <input
        value={draftName}
        onChange={(e) => handleNameChange(e.target.value)}
        onBlur={() => setDraftName(name)}
        className={`w-20 min-w-0 bg-transparent outline-none ${nameInvalid ? "text-st-failed" : "text-fg"}`}
        style={{ fontSize: "11px" }}
        aria-label="Variable name"
        aria-invalid={nameInvalid}
        title={nameInvalid ? (draftName.trim() === "" ? "A variable needs a name" : "Another variable already has this name") : undefined}
        data-testid="pipeline-variable-name"
      />
      <select
        value={def.type}
        onChange={(e) => onUpdate(name, { type: e.target.value as VariableType })}
        className="rounded border border-line-strong bg-bg-4 px-1 py-0.5 text-fg-3 outline-none"
        style={{ fontSize: "10px" }}
        aria-label="Variable type"
        data-testid="pipeline-variable-type"
      >
        {VAR_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
      </select>
      <input
        value={defaultStr}
        onChange={(e) => handleDefaultChange(e.target.value)}
        className="min-w-0 flex-1 bg-transparent text-fg outline-none"
        style={{ fontSize: "11px" }}
        placeholder="default"
        aria-label="Variable default"
        data-testid="pipeline-variable-default"
      />
      <button
        onClick={onDelete}
        className="cursor-pointer text-fg-4 hover:text-st-failed"
        aria-label={`Delete variable ${name}`}
        data-testid="pipeline-variable-delete"
      >
        ×
      </button>
    </div>
  );
}
