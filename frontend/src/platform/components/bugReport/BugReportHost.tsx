import { useBugReport } from "../../store/bugReport";
import BugReportModal from "./BugReportModal";

/** The Bug report window, while it is open (Ctrl+K, Settings › Diagnostics). */
export default function BugReportHost() {
  const opened = useBugReport((s) => s.opened);
  const close = useBugReport((s) => s.closeBugReport);
  return opened ? <BugReportModal prefill={opened} onClose={close} /> : null;
}
