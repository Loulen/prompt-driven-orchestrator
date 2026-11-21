import { useEffect } from "react";
import { useSupportAgent } from "../../store/supportAgentStore";
import SupportAgentDialog from "./SupportAgentDialog";
import SupportAgentTerminal from "./SupportAgentTerminal";
import SupportStillThere from "./SupportStillThere";
import "./supportAgent.css";

/**
 * The Agent de support of the app: its « Open support agent » window, its
 * enlarged terminal and « Still there? », mounted once. It reads, as the page
 * loads, whether the caller's agent lives: a reload finds it again.
 */
export default function SupportAgent() {
  const refresh = useSupportAgent((s) => s.refresh);
  const dialog = useSupportAgent((s) => s.dialog);
  const status = useSupportAgent((s) => s.status);
  const expanded = useSupportAgent((s) => s.expanded);
  const stillThere = useSupportAgent((s) => s.stillThere);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  return (
    <>
      {expanded && status?.alive && <SupportAgentTerminal session={status.session} agent={status.agent} />}
      {dialog && <SupportAgentDialog question={dialog.question} />}
      {stillThere && status?.alive && (
        <SupportStillThere asked={stillThere} harness={status.agent?.harness ?? ""} />
      )}
    </>
  );
}
