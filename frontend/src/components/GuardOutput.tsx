interface GuardOutputProps {
  stdout?: string | null;
  stderr?: string | null;
  /** `!= null` renders the exit-code row; a legit `0` still shows. */
  exitCode?: number | null;
  /** Forwarded onto the root container so callers can keep their own testid
   * contract (TriggerDetailPanel passes `fire-guard-output`, #244; the
   * New-trigger dry-run passes `guard-test-output`, #350). */
  "data-testid"?: string;
}

/**
 * A read-only view of a captured guard run's diagnostics: exit code plus the
 * stdout / stderr streams, each omitted when empty (after `.trim()`). Extracted
 * verbatim from `TriggerDetailPanel` (#244) so the New-trigger dry-run (#350) and
 * the trigger detail panel (#351) render the same surface. Purely presentational
 * — no fetching, no live streaming.
 */
export default function GuardOutput({
  stdout,
  stderr,
  exitCode,
  "data-testid": testId,
}: GuardOutputProps) {
  return (
    <div className="flex flex-col gap-1 pt-0.5" data-testid={testId}>
      {exitCode != null && (
        <div className="text-fg-4" style={{ fontSize: "10px" }}>
          exit code <span className="font-mono text-fg-2">{exitCode}</span>
        </div>
      )}
      {stdout?.trim() && <GuardStream label="stdout" text={stdout} />}
      {stderr?.trim() && <GuardStream label="stderr" text={stderr} />}
    </div>
  );
}

/** A labelled, scrollable block for one captured guard stream (#244). */
function GuardStream({ label, text }: { label: string; text: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <div className="text-fg-4" style={{ fontSize: "10px" }}>
        {label}
      </div>
      <pre
        className="overflow-x-auto whitespace-pre-wrap rounded border border-line bg-bg-0 px-2 py-1.5 font-mono text-fg-3"
        style={{ fontSize: "10px" }}
      >
        {text}
      </pre>
    </div>
  );
}
