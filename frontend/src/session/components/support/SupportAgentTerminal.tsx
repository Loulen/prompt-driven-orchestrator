import { useCallback } from "react";
import { Power } from "lucide-react";
import type { SupportAgentView } from "../../../shared/generated/SupportAgentView";
import { Button } from "../../../shared/ui/kit";
import HarnessLogo from "../../../harness/components/HarnessLogo";
import { harnessColor } from "../../../harness/model/color";
import { useEnlargedTerminal } from "../../../run/components/nodeDetail/useEnlargedTerminal";
import SessionTerminal from "../SessionTerminal";
import { useSupportAgent } from "../../store/supportAgentStore";
import { agentTag } from "../../model/supportAgent";

/** The enlarged terminal's drop shadow and backdrop, as a node's (#44). */
const OVERLAY_SHADOW = "0 30px 80px var(--shadow)";
const OVERLAY_BACKDROP = { background: "var(--dim)", backdropFilter: "blur(4px)" } as const;

/**
 * The live Agent de support in the « Terminal agrandi », over the whole app:
 * its harness, « Support agent » and its combination at the head of the bar;
 * Minimize (or Escape, or the backdrop) hides it and the agent goes on; End
 * session ends it, without confirmation.
 */
export default function SupportAgentTerminal({ session, agent }: { session: string; agent: SupportAgentView | null }) {
  const collapse = useSupportAgent((s) => s.collapse);
  const end = useSupportAgent((s) => s.end);
  const pending = useSupportAgent((s) => s.pending);
  const stableCollapse = useCallback(() => collapse(), [collapse]);
  const { frameRef, onFramePointerDownCapture, onBackdropPointerDown, onBackdropPointerUp } = useEnlargedTerminal(
    true,
    stableCollapse,
  );
  const harness = agent?.harness ?? "";
  return (
    <>
      <div
        className="fixed inset-0 z-40"
        style={OVERLAY_BACKDROP}
        data-testid="support-agent-backdrop"
        onPointerDown={onBackdropPointerDown}
        onPointerUp={onBackdropPointerUp}
      />
      <div
        ref={frameRef}
        className="fixed inset-4 z-[41] flex flex-col overflow-hidden rounded border"
        style={{
          borderColor: harness ? `color-mix(in oklab, ${harnessColor(harness)} 50%, var(--line))` : "var(--line)",
          boxShadow: OVERLAY_SHADOW,
        }}
        role="dialog"
        aria-modal
        aria-label="Support agent"
        data-testid="support-agent-terminal"
        onPointerDownCapture={onFramePointerDownCapture}
      >
        <SessionTerminal
          session={session}
          expanded
          onExpand={stableCollapse}
          toolbarIdentity={
            <span className="sa-ident" data-testid="support-agent-identity">
              {harness && <HarnessLogo harness={harness} size={16} />}
              <span className="sa-ident__name">Support agent</span>
              {agent && <span className="sa-ident__tag">{agentTag(agent)}</span>}
            </span>
          }
          toolbarActions={
            <Button
              tone="ghost"
              size="sm"
              icon={Power}
              disabled={pending}
              onClick={() => void end()}
              data-testid="support-agent-end"
            >
              End session
            </Button>
          }
        />
      </div>
    </>
  );
}
