import { CircleHelp } from "lucide-react";
import { Tooltip } from "../../../shared/ui/kit";
import HarnessLogo from "../../../harness/components/HarnessLogo";
import { useSupportAgent, useSupportAgentUnread } from "../../store/supportAgentStore";

/**
 * The Agent de support's entry, at the right end of the status bar: the logo
 * of its harness while one lives, which brings its terminal back; « ? »
 * otherwise, which opens the « Open support agent » window. An amber dot says
 * it answered since its terminal was minimized.
 */
export default function SupportAgentEntry() {
  const status = useSupportAgent((s) => s.status);
  const unread = useSupportAgentUnread();
  const harness = status?.alive ? status.agent?.harness : undefined;
  const label = !harness ? "Ask the support agent" : unread ? "Support agent: new answer" : "Support agent";
  const onClick = () => {
    const store = useSupportAgent.getState();
    if (harness) store.expand();
    else store.showDialog();
  };
  return (
    <Tooltip label={label} side="top">
      <button
        type="button"
        className="sh-sb__item sa-entry"
        aria-label={label}
        onClick={onClick}
        data-state={harness ? "open" : "none"}
        data-testid="support-agent-entry"
      >
        {harness ? (
          <HarnessLogo harness={harness} size={13} />
        ) : (
          <CircleHelp size={13} strokeWidth={1.5} aria-hidden />
        )}
        {unread && <span className="sa-entry__dot" aria-hidden data-testid="support-agent-unread" />}
      </button>
    </Tooltip>
  );
}
