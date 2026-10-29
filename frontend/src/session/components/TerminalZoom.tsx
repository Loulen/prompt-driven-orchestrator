import { Minus, Plus } from "lucide-react";
import { IconButton, Segmented, toast } from "../../shared/ui/kit";
import { useTerminalZoom } from "../store/useTerminalZoom";
import { stepTerminalZoom, TERMINAL_ZOOMS } from "../model/terminalZoom";
import "./sessionTerminal.css";

/** Why the control is off for a session that carries no Zoom du terminal. */
export const NOT_HOSTED_HINT = "Only a hosted session has a terminal zoom";

/** The steps, plus `zoom` in its place when it is between two (set by the
 *  Host or the API): the live one is always a segment. */
function stepsWith(zoom: number) {
  const zooms: number[] = TERMINAL_ZOOMS.includes(zoom as (typeof TERMINAL_ZOOMS)[number])
    ? [...TERMINAL_ZOOMS]
    : [...TERMINAL_ZOOMS, zoom].sort((a, b) => a - b);
  return zooms.map((z) => ({ value: String(z), label: `${z}%` }));
}

/**
 * The Zoom du terminal of a Session hébergée (CORE-124): its steps from 50 %
 * to 200 %, between − and +. A change applies to every Poste on the session,
 * which the hint says. Off for a session that carries none.
 */
export default function TerminalZoom({ session }: { session: string }) {
  const { zoom, hosted, setZoom } = useTerminalZoom(session);
  const off = hosted !== true;
  const change = (next: number) => {
    if (next === zoom) return;
    setZoom(next).catch((e: unknown) =>
      toast(e instanceof Error ? e.message : String(e), "error", { title: "Terminal zoom" }),
    );
  };
  return (
    <div className="ri-term-zoom" data-testid="terminal-zoom" data-zoom={zoom} title={hosted === false ? NOT_HOSTED_HINT : undefined}>
      <div className="ri-term-zoom__head">
        <span className="ri-term-zoom__title">Terminal zoom</span>
        <span className="ri-term-zoom__hint">same for everyone watching</span>
      </div>
      <div className="ri-term-zoom__row">
        <IconButton
          icon={Minus}
          label="Zoom out"
          size="sm"
          disabled={off || stepTerminalZoom(zoom, -1) === zoom}
          onClick={() => change(stepTerminalZoom(zoom, -1))}
          data-testid="terminal-zoom-out"
        />
        <Segmented
          label="Terminal zoom"
          size="sm"
          items={stepsWith(zoom)}
          value={String(zoom)}
          disabled={off}
          onChange={(value) => change(Number(value))}
        />
        <IconButton
          icon={Plus}
          label="Zoom in"
          size="sm"
          disabled={off || stepTerminalZoom(zoom, 1) === zoom}
          onClick={() => change(stepTerminalZoom(zoom, 1))}
          data-testid="terminal-zoom-in"
        />
      </div>
    </div>
  );
}
