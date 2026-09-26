import type { PortSide } from "../shared/generated/PortSide";

const SIDES: PortSide[] = ["left", "top", "right", "bottom"];

const SIDE_LABELS: Record<PortSide, string> = {
  left: "L",
  right: "R",
  top: "T",
  bottom: "B",
};

export default function SidePicker({
  value,
  onChange,
}: {
  value: PortSide;
  onChange: (s: PortSide) => void;
}) {
  return (
    <div className="flex rounded border border-line-strong overflow-hidden" title="Handle side">
      {SIDES.map((s) => (
        <button
          key={s}
          onClick={() => onChange(s)}
          className={`cursor-pointer px-1.5 py-px transition-colors ${
            value === s
              ? "bg-acc-bg text-acc font-medium"
              : "bg-bg-4 text-fg-4 hover:text-fg-3"
          }`}
          style={{ fontSize: "9px", lineHeight: "1.4" }}
          title={s}
        >
          {SIDE_LABELS[s]}
        </button>
      ))}
    </div>
  );
}
