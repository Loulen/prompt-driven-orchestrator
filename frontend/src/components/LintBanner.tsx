import { Info } from "lucide-react";

interface Props {
  diagnostics: string[];
}

export default function LintBanner({ diagnostics }: Props) {
  if (diagnostics.length === 0) return null;

  return (
    <div
      data-testid="lint-banner"
      className="mx-3 mt-2 flex flex-col gap-1"
    >
      {diagnostics.map((msg, i) => (
        <div
          key={i}
          className="flex items-start gap-2 rounded border border-st-await/30 bg-st-await/5 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          <Info size={12} className="mt-0.5 shrink-0 text-st-await" />
          <span>{msg}</span>
        </div>
      ))}
    </div>
  );
}
