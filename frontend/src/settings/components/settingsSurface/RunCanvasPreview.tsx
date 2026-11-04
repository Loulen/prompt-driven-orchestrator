import { Fragment, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AppearancePreviewContext, useElementMarks } from "../../../hooks/useElementMarks";
import { deriveEditNodes } from "../../../pipeline/components/editNodeDerivation";
import type { EditNodeData } from "../../../pipeline/components/EditNode";
import { WorkNodeCard } from "../../../pipeline/components/WorkNodeCard";
import { childrenOfNode, countChildren, useRunChildren } from "../../../run/model/orchestration";
import { useAgentProfiles } from "../../store/useAgentProfiles";
import { canvasPreviewRows, SAMPLE_CARDS } from "../../model/runCanvasPreview";
import type { RecentRun } from "../../store/useRecentRun";
import "../../../pipeline/components/canvas.css";

/** The preview's two columns, in px: a fork's cards sit at their middles, a single card at the middle of both. */
const COLUMN = 222;
const WIDTH = 2 * COLUMN;
const GAP = 24;
const KNEE = 10;
const LEFT = COLUMN / 2;
const RIGHT = COLUMN + COLUMN / 2;
const MIDDLE = COLUMN;

/** An arrowhead landing at (`x`, `y`), pointing down. */
const arrow = (x: number) => `${x - 3.5},${GAP - 6} ${x + 3.5},${GAP - 6} ${x},${GAP}`;

/**
 * The wires between two rows, square like the canvas's: straight down from one
 * card to the next, a fork from one to two, a join from two to one. Drawn in the
 * flow between the rows, so a row that shortens takes its wires up with it, straight.
 */
function Wires({ from, to }: { from: number; to: number }) {
  const ends = to === 2 ? [LEFT, RIGHT] : [MIDDLE];
  const path =
    from === 1 && to === 1
      ? `M${MIDDLE},0 V${GAP}`
      : from === 1
        ? `M${MIDDLE},0 V${KNEE} M${LEFT},${KNEE} H${RIGHT} M${LEFT},${KNEE} V${GAP} M${RIGHT},${KNEE} V${GAP}`
        : to === 1
          ? `M${LEFT},0 V${KNEE} M${RIGHT},0 V${KNEE} M${LEFT},${KNEE} H${RIGHT} M${MIDDLE},${KNEE} V${GAP}`
          : `M${LEFT},0 V${GAP} M${RIGHT},0 V${GAP}`;
  return (
    <svg className="set-preview__wires" width={WIDTH} height={GAP} viewBox={`0 0 ${WIDTH} ${GAP}`} aria-hidden>
      <path d={path} />
      {ends.map((x) => (
        <polygon key={x} points={arrow(x)} />
      ))}
    </svg>
  );
}

/**
 * The zoom that fits the preview's two columns (`WIDTH`) in its frame: 1 while the
 * frame keeps its 470 px, less when the block's column is narrower (a laptop
 * screen), so a fork never overflows the frame.
 */
function useFitZoom() {
  const frame = useRef<HTMLDivElement>(null);
  const [zoom, setZoom] = useState(1);
  useLayoutEffect(() => {
    const element = frame.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const fit = () => {
      const style = getComputedStyle(element);
      const inner = element.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      setZoom(inner > 0 ? Math.min(1, inner / WIDTH) : 1);
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return { frame, zoom };
}

/** The cards of the most recent Run, as its canvas derives them, with its orchestrators' child runs. */
function useRecentCards(recent: RecentRun | null): EditNodeData[] | null {
  const { profiles } = useAgentProfiles(recent != null);
  const runId = recent?.run.run_id ?? null;
  const children = useRunChildren(runId, recent != null);
  return useMemo(() => {
    if (!recent?.pipeline) return null;
    const ids = new Set(profiles.map((profile) => profile.id));
    return deriveEditNodes(recent.pipeline, recent.run, ids, profiles).map((node) => {
      const data = node.data as EditNodeData;
      const kids = data.orchestrator ? childrenOfNode(children, recent.run.run_id, node.id) : [];
      return kids.length > 0 ? { ...data, childCounts: countChildren(kids) } : data;
    });
  }, [recent, profiles, children]);
}

function Cards({ rows, compact }: { rows: EditNodeData[][]; compact: boolean }) {
  const marks = useElementMarks();
  return (
    <>
      {rows.map((row, index) => (
        <Fragment key={row.map((card) => card.nodeId).join(" ")}>
          {index > 0 && <Wires from={rows[index - 1].length} to={row.length} />}
          <div className="set-preview__row" data-fork={row.length === 2 || undefined}>
            {row.map((card) => (
              <WorkNodeCard key={card.nodeId} data={{ ...card, compact, onOpenOrchestration: undefined }} marks={marks} />
            ))}
          </div>
        </Fragment>
      ))}
    </>
  );
}

/**
 * The preview of the Run canvas block: the canvas's own cards (`WorkNodeCard`) for a
 * few nodes of the most recent Run the user sees, with a fork, at 470 px (zoomed out to fit a narrower column); on a sample
 * Run when they see none. Detailed or on one line as « Compact nodes » says, each card
 * at the height of what the Apparence shows. Inert: nothing in it reacts. `pointed` is
 * the element whose switch the pointer or the focus is on.
 */
export default function RunCanvasPreview({
  recent,
  none,
  compact,
  pointed,
}: {
  recent: RecentRun | null;
  /** The user sees no Run. */
  none: boolean;
  compact: boolean;
  pointed: string | null;
}) {
  const cards = useRecentCards(recent);
  const { frame, zoom } = useFitZoom();
  const sample = cards == null || cards.length === 0;
  const rows = useMemo(() => canvasPreviewRows(sample ? SAMPLE_CARDS : cards), [sample, cards]);
  const context = useMemo(() => ({ pointed, sample }), [pointed, sample]);
  const caption = !sample && recent
    ? `Your most recent run · ${recent.run.pipeline_name}`
    : none
      ? "Sample run · you see no run yet"
      : "Sample run";
  return (
    <AppearancePreviewContext.Provider value={context}>
      <figure
        className="set-preview"
        data-testid="appearance-preview-run-canvas"
        data-compact={compact || undefined}
        data-sample={sample || undefined}
      >
        <div className="set-preview__canvas" ref={frame} inert>
          <div className="set-preview__stage" style={zoom < 1 ? { zoom } : undefined}>
            <Cards rows={rows} compact={compact} />
          </div>
        </div>
        <figcaption className="set-preview__caption">{caption}</figcaption>
      </figure>
    </AppearancePreviewContext.Provider>
  );
}
