import { ChevronDown } from "lucide-react";
import type { IterationInfo } from "../../../shared/generated/IterationInfo";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../../../shared/ui/dropdown-menu";
import { useNodeDurationMode } from "../../store/nodeDurationModeStore";
import { IterOptionDuration } from "./IterationDuration";
import { StatusIcon } from "../../../shared/ui/kit";

export default function IterSelector({
  iterations,
  selectedIter,
  onSelect,
}: {
  iterations: IterationInfo[];
  selectedIter: number;
  onSelect: (iter: number) => void;
}) {
  // Each iteration's duration reads in the Mode de durée on screen.
  const durationMode = useNodeDurationMode((st) => st.mode);
  // The latest iteration first: it is the one the user usually looks for.
  const latestFirst = [...iterations].sort((a, b) => b.iter - a.iter);
  const total = iterations.length;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="ri-iter" aria-label={`Iteration ${selectedIter} of ${total}`} title="Iteration">
        i{selectedIter}/{total}
        <ChevronDown size={11} strokeWidth={1.5} aria-hidden />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="ri-iter-menu" side="bottom" align="end">
        {latestFirst.map((it) => (
          <DropdownMenuItem
            key={it.iter}
            className="ri-iter-menu__item"
            data-on={it.iter === selectedIter || undefined}
            onClick={() => onSelect(it.iter)}
            data-testid={`iter-option-${it.iter}`}
          >
            <StatusIcon kind="node" status={it.status} size="sm" />
            <span className="font-mono">iter {it.iter}</span>
            <span className="ri-iter-menu__dur">
              <IterOptionDuration timing={it} mode={durationMode} />
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
