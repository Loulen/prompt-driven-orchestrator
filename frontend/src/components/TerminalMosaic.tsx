import { Fragment, useEffect, useRef, type ReactNode } from "react";
import TmuxTerminal from "./TmuxTerminal";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "./ui/resizable";
import { mosaicRows, paneTitle, type TerminalRef } from "../lib/terminalMosaic";
import type { PaneFocusRequest } from "../hooks/useTerminalMosaic";

/** A pane stays wide and tall enough to read a few lines. No minimum when it is alone. */
const PANE_MIN = "120px";

interface Props {
  /** The node's own terminal. Always at the same place in the tree. */
  primary: ReactNode;
  primarySession: string;
  /** The panes added around it (empty outside the expanded view). */
  extras: TerminalRef[];
  statuses: Record<string, string | undefined>;
  onRemove: (session: string) => void;
  focusRequest: PaneFocusRequest | null;
}

/**
 * #1040 — the Mosaïque's grid: the primary terminal, then up to three added
 * panes (side by side, columns, then 2×2), resizable.
 *
 * The structure is the same with no added pane — in the split view too — so the
 * primary `TmuxTerminal` keeps its tree position whatever joins or leaves: rows
 * group › first row › columns group › first panel › primary. Added panes are
 * only siblings after it; React never remounts the primary, so its xterm and
 * WebSocket survive (the #968 invariant). Going from three panes to four moves
 * the third into the second row: that pane, not the primary, reconnects.
 */
export default function TerminalMosaic({
  primary,
  primarySession,
  extras,
  statuses,
  onRemove,
  focusRequest,
}: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [firstRow, secondRow = []] = mosaicRows<TerminalRef | "primary">(["primary", ...extras]);
  const isAdded = (p: TerminalRef | "primary"): p is TerminalRef => p !== "primary";
  const firstRowExtras = firstRow.filter(isAdded);
  const secondRowExtras = secondRow.filter(isAdded);

  // The keyboard goes to the pane just added — or to the one already showing
  // the session asked for.
  useEffect(() => {
    if (!focusRequest) return;
    const panes = rootRef.current?.querySelectorAll<HTMLElement>("[data-mosaic-pane]") ?? [];
    const pane = Array.from(panes).find((p) => p.dataset.mosaicPane === focusRequest.session);
    pane?.querySelector<HTMLTextAreaElement>(".xterm-helper-textarea")?.focus();
  }, [focusRequest]);

  // A pane added around the primary; `handle` puts a separator before it.
  const extraPane = (ref: TerminalRef, handle: boolean) => (
    <Fragment key={ref.session}>
      {handle && <ResizableHandle />}
      <ResizablePanel id={`mosaic-${ref.session}`} minSize={PANE_MIN}>
        <div
          className="flex h-full flex-col overflow-hidden"
          data-mosaic-pane={ref.session}
          data-testid="mosaic-pane"
        >
          <TmuxTerminal
            session={ref.session}
            title={paneTitle(ref)}
            status={ref.kind === "node" ? statuses[ref.session] : undefined}
            paneSource={ref.paneSource}
            onRemove={() => onRemove(ref.session)}
          />
        </div>
      </ResizablePanel>
    </Fragment>
  );

  return (
    <div ref={rootRef} className="flex min-h-0 flex-1 flex-col" data-testid="terminal-mosaic" data-panes={1 + extras.length}>
      <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1">
        <ResizablePanel id="mosaic-row-1" minSize={secondRowExtras.length > 0 ? PANE_MIN : undefined}>
          <ResizablePanelGroup orientation="horizontal">
            <ResizablePanel id="mosaic-primary" minSize={firstRowExtras.length > 0 ? PANE_MIN : undefined}>
              <div className="flex h-full flex-col overflow-hidden" data-mosaic-pane={primarySession}>
                {primary}
              </div>
            </ResizablePanel>
            {firstRowExtras.map((ref) => extraPane(ref, true))}
          </ResizablePanelGroup>
        </ResizablePanel>
        {secondRowExtras.length > 0 && (
          <>
            <ResizableHandle />
            <ResizablePanel id="mosaic-row-2" minSize={PANE_MIN}>
              <ResizablePanelGroup orientation="horizontal">
                {secondRowExtras.map((ref, i) => extraPane(ref, i > 0))}
              </ResizablePanelGroup>
            </ResizablePanel>
          </>
        )}
      </ResizablePanelGroup>
    </div>
  );
}
