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
 * stdout / stderr streams, each omitted when empty (after `.trim()`). Shared by
 * the New-trigger dry-run (#350) and the trigger detail panel (#244, #351).
 * Purely presentational — no fetching, no live streaming.
 */
export default function GuardOutput({
  stdout,
  stderr,
  exitCode,
  "data-testid": testId,
}: GuardOutputProps) {
  const out = stdout?.trim();
  const err = stderr?.trim();
  if (exitCode == null && !out && !err) return null;
  return (
    <div className="tg-streams" data-testid={testId}>
      {exitCode != null && (
        <div className="tg-faint">
          exit <span className="tg-mono">{exitCode}</span>
        </div>
      )}
      {out && <GuardStream label="stdout" text={stdout!} />}
      {err && <GuardStream label="stderr" text={stderr!} />}
    </div>
  );
}

/** One captured guard stream (#244), scrollable. */
function GuardStream({ label, text }: { label: string; text: string }) {
  return (
    <div className="tg-stream">
      <span className="d-caps tg-stream__label">{label}</span>
      <pre className="tg-pre">{text}</pre>
    </div>
  );
}
