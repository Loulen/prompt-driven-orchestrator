import { useState } from "react";
import { ChevronDown, ChevronRight, Info, Plus, Radio } from "lucide-react";
import type { LiveArtifact } from "../../../shared/generated/LiveArtifact";
import type { PortType } from "../../../shared/generated/PortType";
import { EmptyState, IconButton, Section } from "../../../shared/ui/kit";
import InspectorPortRow, { PortRenameNotices } from "../InspectorPortRow";
import ExpectedContentField from "../ExpectedContentField";
import OutputSchemaEditor from "../OutputSchemaEditor";
import { PORT_TYPE_OPTIONS } from "../portTypeOptions";
import { usePortNameDraft } from "../usePortNameDraft";

const FREE_HINT = "Free live artifacts: undeclared files in live/, shown during the Run";

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

  return (
    <Section
      title="Live artifacts"
      count={live.length}
      flush
      aside={
        <span className="ei-keep">
          <span className="d-meta" title={FREE_HINT}>
            Free
          </span>
          <button
            type="button"
            role="switch"
            aria-checked={free}
            aria-label="Free live artifacts"
            title={FREE_HINT}
            disabled={readOnly}
            data-testid="free-live-artifacts-toggle"
            onClick={() => onChange({ free_live_artifacts: !free })}
            className="d-switch"
          >
            <span className="d-switch__knob" aria-hidden />
          </button>
          <span role="img" aria-label="About free live artifacts" title={FREE_HINT} className="ei-dim" data-testid="free-live-artifacts-info">
            <Info size={11} strokeWidth={1.5} aria-hidden />
          </span>
          {!readOnly && (
            <IconButton icon={Plus} size="sm" label="Add live artifact" onClick={add} data-testid="add-live-artifact" />
          )}
        </span>
      }
    >
      <div data-testid="live-artifacts" title="Announced to the agent, never checked; shown in the Run while the node works">
        {live.length === 0 && (
          <EmptyState icon={Radio} layout="row">
            None declared
          </EmptyState>
        )}
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
    </Section>
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
        data-collapsed={collapsed || undefined}
        className="ei-port"
      >
        <div className="ei-port__head">
          <IconButton
            icon={collapsed ? ChevronRight : ChevronDown}
            size="sm"
            label="Toggle live artifact body"
            aria-expanded={!collapsed}
            onClick={() => setCollapsed((c) => !c)}
          />
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
              className="ei-select ei-select--sm ei-select--mono"
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
        {!collapsed && (allowInstructions || portType === "markdown") && (
          <div className="ei-port__body">
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
      <div className="ei-port__notices">
        <PortRenameNotices portName={artifact.name} draft={nameDraft} />
      </div>
    </>
  );
}
