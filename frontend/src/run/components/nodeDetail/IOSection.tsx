import type { PortIO, FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import PortRow from "./PortRow";

export default function IOSection({
  title,
  kind,
  ports,
  runId,
  nodeId,
  showFrontmatter,
  onOpenFile,
}: {
  title: string;
  /** #825: what a row of this section is, on the row itself — the *First run*
   *  tour points at `out` under Outputs, and `out` also exists under Inputs. */
  kind: "input" | "output";
  ports: PortIO[];
  runId: string;
  /** The node the ports belong to (the `port/item` decorations' context). */
  nodeId?: string;
  showFrontmatter?: boolean;
  onOpenFile: (portName: string, files: FileInfo[], portType: PortType) => void;
}) {
  return (
    <div className="border-t border-line">
      <div
        className="flex items-center gap-1.5 px-3 py-1.5 text-text-3"
        style={{ fontSize: "11px" }}
      >
        {title}
        <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
          {ports.length}
        </span>
      </div>
      <div className="flex flex-col gap-1 px-3 pb-2">
        {ports.map((port) => (
          <PortRow
            key={port.port}
            port={port}
            kind={kind}
            runId={runId}
            nodeId={nodeId}
            showFrontmatter={showFrontmatter}
            onOpen={() => onOpenFile(port.port, port.files, port.port_type ?? "markdown")}
          />
        ))}
      </div>
    </div>
  );
}
