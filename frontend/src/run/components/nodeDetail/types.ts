import type { FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";

// The terminal inset has three mutually exclusive display modes (#346):
//   - "split"     : terminal ~45% / detail pane ~55% (default for a live node)
//   - "expanded"  : terminal full-frame, detail pane hidden (user gesture, #270)
//   - "minimized" : terminal collapsed to a thin bar, Outputs take the full
//                   height (default when opening a node whose session ended)
// An enum (not two orthogonal booleans) makes the illegal
// `{minimized + expanded}` state unrepresentable.
export type TerminalView = "minimized" | "split" | "expanded";

export interface ModalState {
  portName: string;
  files: FileInfo[];
  portKind: "input" | "output";
  portType: PortType;
}
