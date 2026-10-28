import { useNodeSearchMatches } from "../../store/runSearchStore";

/** On a node card of the Run's canvas, while the search is open: how many
 *  entries of the node's transcripts match. Nothing without a match. */
export default function SearchMatchBadge({ nodeId }: { nodeId: string }) {
  const matched = useNodeSearchMatches(nodeId);
  if (matched === 0) return null;
  return (
    <span
      data-testid="node-search-matches"
      title={`${matched} ${matched === 1 ? "entry matches" : "entries match"} the search`}
      className="absolute -right-2 -top-2 z-[4] box-border grid h-[16px] min-w-[16px] place-items-center rounded-[8px] border-2 border-surface-0 bg-st-await px-1 font-sans font-semibold leading-none text-primary-ink"
      style={{ fontSize: "9.5px" }}
    >
      {matched}
    </span>
  );
}
