import type { PortDef } from "../../../shared/generated/PortDef";
import type { EditorNodeDef, EditorPipelineDef } from "../../model/pipeline";
import { outputRenameImpact } from "../../../lib/edgePorts";
import { SectionHead } from "../InspectorPrimitives";
import OutputPortCard from "../OutputPortCard";

/** The node's declared outputs: its production contract (CONTEXT.md § Node). */
export default function OutputsSection({
  node,
  pipeline,
  highlightedPort,
  onChange,
}: {
  node: EditorNodeDef;
  pipeline: EditorPipelineDef;
  highlightedPort: string | null;
  onChange: (outputs: PortDef[]) => void;
}) {
  function add() {
    const ports = [...node.outputs];
    let name = "out";
    let counter = 1;
    while (ports.some((p) => p.name === name)) {
      name = `out-${++counter}`;
    }
    ports.push({ name });
    onChange(ports);
  }

  function update(index: number, updates: Partial<PortDef>) {
    onChange(node.outputs.map((p, i) => (i === index ? { ...p, ...updates } : p)));
  }

  function remove(index: number) {
    onChange(node.outputs.filter((_, i) => i !== index));
  }

  return (
    <>
      <SectionHead
        title="Outputs"
        count={node.outputs.length}
        onAdd={add}
        addTestId="add-output-port"
      />
      <div className="flex flex-col">
        {node.outputs.map((port, i) => (
          <OutputPortCard
            key={i}
            index={i}
            port={port}
            highlighted={highlightedPort === port.name}
            onUpdate={(updates) => update(i, updates)}
            onRemove={() => remove(i)}
            schema={port.frontmatter}
            onSchemaChange={(fm) => update(i, { frontmatter: fm ?? null })}
            allowInstructions={node.type === "agent"}
            takenNames={node.outputs.filter((_, j) => j !== i).map((p) => p.name)}
            wiring={outputRenameImpact(pipeline, node.id, port.name)}
          />
        ))}
      </div>
    </>
  );
}
