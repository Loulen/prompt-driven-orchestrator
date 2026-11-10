import { Circle, CircleCheck, Square } from "lucide-react";
import type { EndNodeInfo } from "../../shared/generated/EndNodeInfo";
import { Chip, EmptyState, Section } from "../../shared/ui/kit";
import "./inspect.css";

interface Props {
  endNode: EndNodeInfo;
}

/** The Run's End: each of its ports, received or pending, and the reason it ended. */
export default function EndInspector({ endNode }: Props) {
  return (
    <aside className="end-inspector ei" data-testid="end-inspector">
      <div className="ei-bar">
        <span className="ei-bar__title">
          <Square size={14} strokeWidth={1.5} aria-hidden />
          <span>Run end</span>
          <span className="ei-bar__sub">{endNode.id}</span>
        </span>
        <span className="ei-bar__actions">
          <span className="runtime-badge" title="Filled in by the runtime">
            <Chip>runtime</Chip>
          </span>
        </span>
      </div>

      <Section title="Termination reasons" count={endNode.ports.length} flush>
        {endNode.ports.length === 0 && (
          <EmptyState icon={Circle} layout="row">
            None
          </EmptyState>
        )}
        {endNode.ports.map((port) => (
          <div key={port.port_name} data-testid={`end-port-${port.port_name}`}>
            <div className="ei-row" title={port.reason ?? port.port_name}>
              {port.status === "received" ? (
                <CircleCheck size={12} strokeWidth={2} aria-hidden style={{ color: "var(--color-st-done)", flex: "none" }} />
              ) : (
                <Circle size={12} strokeWidth={1.5} aria-hidden className="ei-ic" />
              )}
              <span className="ei-row__name d-mono">{port.port_name}</span>
              <span className="ei-row__end">
                <Chip tone={port.status === "received" ? "done" : "neutral"}>{port.status}</Chip>
              </span>
            </div>
            {port.reason && <div className="ei-reason">{port.reason}</div>}
          </div>
        ))}
      </Section>
    </aside>
  );
}
