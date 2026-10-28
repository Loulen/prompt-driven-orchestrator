import { TriangleAlert } from "lucide-react";

/**
 * A teardown that failed (#911), said on the card showing at that moment. The
 * daemon's sentence is quoted: « could not be removed » alone would leave the
 * reader nothing to act on.
 */
export default function TourTidyUpFailure({
  failure,
}: {
  failure: { title: string; reason: string } | null | undefined;
}) {
  if (!failure) return null;
  return (
    <div
      data-testid="tour-tidyup-failure"
      className="mt-3 rounded border border-st-await/40 bg-st-await/10 p-2 text-text-2"
      style={{ fontSize: "11px", lineHeight: 1.5 }}
    >
      <div className="flex items-center gap-1.5 font-medium text-st-await">
        <TriangleAlert size={12} className="shrink-0" />
        {failure.title}
      </div>
      <pre
        className="mt-1 whitespace-pre-wrap break-words font-mono text-text-3"
        style={{ fontSize: "10.5px" }}
        data-testid="tour-tidyup-reason"
      >
        {failure.reason}
      </pre>
    </div>
  );
}
