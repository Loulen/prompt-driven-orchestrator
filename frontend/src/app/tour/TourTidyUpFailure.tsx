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
    <div data-testid="tour-tidyup-failure" className="tr-tidy">
      <div className="tr-tidy__t">
        <TriangleAlert size={12} strokeWidth={1.5} aria-hidden />
        {failure.title}
      </div>
      <pre data-testid="tour-tidyup-reason">{failure.reason}</pre>
    </div>
  );
}
