import { useState } from "react";
import { ChevronDown, Info } from "lucide-react";
import type { LiveArtifact } from "../../../shared/generated/LiveArtifact";
import type { PortType } from "../../../shared/generated/PortType";
import { Tooltip } from "../../../shared/ui/tooltip";
import { SectionHead } from "../InspectorPrimitives";
import InspectorPortRow, { PortRenameNotices } from "../InspectorPortRow";
import ExpectedContentField from "../ExpectedContentField";
import OutputSchemaEditor from "../OutputSchemaEditor";
import { PORT_TYPE_OPTIONS } from "../portTypeOptions";
import { usePortNameDraft } from "../usePortNameDraft";

const FREE_HINT =
  "The agent may also publish files you did not declare, in live/. Shown during the Run, never checked, never wired.";

/**
 * ADR-0089: the node's live artifacts, apart from its Outputs — what it
 * publishes while it works, for the humans to follow. Declared ones are
 * announced to the agent, never checked; « Free live artifacts » lets it publish
 * others of its own.
 */
export default function LiveArtifactsSection({
  live,
  free,
  allowInstructions,
  readOnly,
  onChange,
}: {
  live: LiveArtifact[];
  free: boolean;
  /** Only an agent reads an expected content. */
  allowInstructions: boolean;
  readOnly?: boolean;
  onChange: (updates: { live_artifacts?: LiveArtifact[]; free_live_artifacts?: boolean }) => void;
}) {
  function add() {
    let name = "progress";
    let counter = 1;
    while (live.some((l) => l.name === name)) {
      name = `progress-${++counter}`;
    }
    onChange({ live_artifacts: [...live, { name }] });
  }

  function update(index: number, updates: Partial<LiveArtifact>) {
    onChange({ live_artifacts: live.map((l, i) => (i === index ? { ...l, ...updates } : l)) });
  }

  function remove(index: number) {
    onChange({ live_artifacts: live.filter((_, i) => i !== index) });
  }

  const freeToggle = (
    <div className="ml-auto mr-2 flex items-center gap-1.5">
      <span className="text-text-3" style={{ fontSize: "10px" }}>
        Free live artifacts
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={free}
        aria-label="Free live artifacts"
        disabled={readOnly}
        data-testid="free-live-artifacts-toggle"
        onClick={() => onChange({ free_live_artifacts: !free })}
        className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
          free ? "bg-primary" : "bg-text-3"
        }`}
      >
        <span
          className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-surface-0 transition-all ${
            free ? "left-3" : "left-0.5"
          }`}
        />
      </button>
      <Tooltip content={FREE_HINT} side="bottom">
        <span
          role="img"
          aria-label="About free live artifacts"
          className="text-text-3"
          data-testid="free-live-artifacts-info"
        >
          <Info size={11} />
        </span>
      </Tooltip>
    </div>
  );

  return (
    <>
      <SectionHead
        title="Live artifacts"
        count={live.length}
        aside={freeToggle}
        onAdd={readOnly ? undefined : add}
        addTestId="add-live-artifact"
      />
      <p className="-mt-2 px-1 text-text-3" style={{ fontSize: "10px" }}>
        Announced to the agent, never checked. Shown in the Run while the node works.
      </p>
      <div className="flex flex-col" data-testid="live-artifacts">
        {live.map((artifact, i) => (
          <LiveArtifactCard
            key={i}
            artifact={artifact}
            takenNames={live.filter((_, j) => j !== i).map((l) => l.name)}
            allowInstructions={allowInstructions}
            onUpdate={(updates) => update(i, updates)}
            onRemove={() => remove(i)}
          />
        ))}
      </div>
    </>
  );
}

function LiveArtifactCard({
  artifact,
  takenNames,
  allowInstructions,
  onUpdate,
  onRemove,
}: {
  artifact: LiveArtifact;
  takenNames: string[];
  allowInstructions: boolean;
  onUpdate: (updates: Partial<LiveArtifact>) => void;
  onRemove: () => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const portType = artifact.port_type ?? "markdown";
  const nameDraft = usePortNameDraft(artifact.name, takenNames, (name) => onUpdate({ name }));

  return (
    <>
      <div
        data-testid={`live-artifact-card-${artifact.name}`}
        className={`op-tab${collapsed ? " collapsed" : ""}`}
      >
        <div className="op-head">
          <button
            className="op-chev"
            aria-label="Toggle live artifact body"
            onClick={() => setCollapsed((c) => !c)}
          >
            <ChevronDown size={14} />
          </button>
          <InspectorPortRow
            port={artifact}
            isLast
            onUpdate={(updates) => {
              if (updates.name !== undefined) onUpdate({ name: updates.name });
            }}
            onRemove={onRemove}
            takenNames={takenNames}
            nameDraft={nameDraft}
          >
            <select
              value={portType}
              onChange={(e) => onUpdate({ port_type: e.target.value as PortType })}
              aria-label="Live artifact type"
              className="shrink-0 rounded border border-line-strong bg-surface-2 px-1.5 py-0.5 font-mono text-text outline-none focus:border-primary"
              style={{ fontSize: "10px" }}
              data-testid="live-artifact-type-select"
            >
              {PORT_TYPE_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value} title={opt.hint}>
                  {opt.label}
                </option>
              ))}
            </select>
          </InspectorPortRow>
        </div>
        {!collapsed && (
          <div className="op-body">
            {allowInstructions && (
              <ExpectedContentField
                value={artifact.instructions}
                onChange={(instructions) => onUpdate({ instructions })}
              />
            )}
            {portType === "markdown" && (
              <OutputSchemaEditor
                schema={artifact.frontmatter}
                onChange={(frontmatter) => onUpdate({ frontmatter: frontmatter ?? null })}
              />
            )}
          </div>
        )}
      </div>
      <div className="-mt-1 mb-3 empty:hidden">
        <PortRenameNotices portName={artifact.name} draft={nameDraft} />
      </div>
    </>
  );
}
