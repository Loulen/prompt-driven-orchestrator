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
import { STATUS_DOTS } from "./labels";

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
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className="flex cursor-pointer items-center gap-0.5 whitespace-nowrap rounded px-1 py-0.5 font-mono text-text-3 transition-colors hover:bg-hover hover:text-text-2"
        style={{ fontSize: "10px" }}
      >
        iter {selectedIter}
        <ChevronDown size={10} className="text-text-3" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className="min-w-[180px] rounded-md border border-line-strong bg-surface-2 p-1 shadow-lg"
        side="bottom"
        align="start"
      >
        {latestFirst.map((it) => (
          <DropdownMenuItem
            key={it.iter}
            className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-text-2 transition-colors hover:bg-hover ${
              it.iter === selectedIter ? "bg-surface-2" : ""
            }`}
            style={{ fontSize: "11px" }}
            onClick={() => onSelect(it.iter)}
            data-testid={`iter-option-${it.iter}`}
          >
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATUS_DOTS[it.status]}`}
            />
            <span className="font-mono">iter {it.iter}</span>
            <span
              className="ml-auto font-mono text-text-3"
              style={{ fontSize: "10px" }}
            >
              <IterOptionDuration timing={it} mode={durationMode} />
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
