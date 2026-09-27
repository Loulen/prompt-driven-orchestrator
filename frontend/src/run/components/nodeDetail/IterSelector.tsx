import { ChevronDown } from "lucide-react";
import type { IterationInfo } from "../../../shared/generated/IterationInfo";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../../../shared/ui/dropdown-menu";
import { formatTime } from "../../model/nodeDetailFormat";
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
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className="flex cursor-pointer items-center gap-0.5 rounded px-1 py-0.5 font-mono text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg-2"
        style={{ fontSize: "10px" }}
      >
        iter {selectedIter}
        <ChevronDown size={10} className="text-fg-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className="min-w-[180px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
        side="bottom"
        align="start"
      >
        {iterations.map((it) => (
          <DropdownMenuItem
            key={it.iter}
            className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 ${
              it.iter === selectedIter ? "bg-bg-4" : ""
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
              className="ml-auto font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              {it.started_at ? formatTime(it.started_at) : ""}
              {it.completed_at ? ` – ${formatTime(it.completed_at)}` : ""}
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
