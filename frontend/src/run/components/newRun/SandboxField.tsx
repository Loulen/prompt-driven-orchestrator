import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** The Sandbox selector (#410/#432/#452) and its warnings. */
export default function SandboxField({ form, onManageStagingProfiles }: { form: NewRunForm; onManageStagingProfiles?: () => void }) {
  const {
    mode, settings, settingsFailed, sandbox, setSandbox, dockerUnavailable, sandboxReason,
    sandboxProfiles, missingProfile, instanceDefaultSandbox, effectiveSandbox, sandboxDoomed,
    inheritedDefaultReason,
  } = form;
  return (
  <div className="flex flex-col gap-1.5">
    <div className="flex items-baseline justify-between">
      <label
        htmlFor="sandbox-select"
        className="font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Sandbox
      </label>
      {onManageStagingProfiles && (
        <button
          type="button"
          onClick={onManageStagingProfiles}
          data-testid="new-run-manage-staging-profiles"
          className="text-fg-3 underline-offset-2 hover:text-acc hover:underline"
          style={{ fontSize: "10.5px" }}
        >
          Manage staging profiles…
        </button>
      )}
    </div>
    <select
      id="sandbox-select"
      data-testid="sandbox-select"
      value={sandbox}
      onChange={(e) => setSandbox(e.target.value)}
      title={dockerUnavailable ? sandboxReason : undefined}
      className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none disabled:opacity-40"
      style={{ fontSize: "12px" }}
    >
      {/* #452: run mode names the resolved default, because the choice is made
          NOW and the user is entitled to know what they are inheriting. A Trigger
          resolves when it fires, so naming today's value there would be a
          promise the modal cannot keep. */}
      <option value="">
        {mode === "run" && instanceDefaultSandbox
          ? `Use instance default (${instanceDefaultSandbox})`
          : "Use instance default"}
      </option>
      <option value="off">off (run on the host)</option>
      {sandboxProfiles.map((p) => (
        <option key={p.name} value={p.name} disabled={dockerUnavailable}>
          {dockerUnavailable
            ? `${p.name} (Docker unavailable)`
            : `${p.name} (Docker sandbox)`}
        </option>
      ))}
      {/* Tombstone: keeps the seeded value SELECTED (React would otherwise
          render the field blank and a save would PATCH `sandbox: null` — a
          silent fallback to the instance default). */}
      {missingProfile && (
        <option value={sandbox} data-testid="sandbox-missing-profile">
          {sandbox} — missing
        </option>
      )}
    </select>
    {missingProfile && (
      <span
        className="text-st-failed"
        style={{ fontSize: "10.5px" }}
        data-testid="sandbox-missing-profile-warning"
      >
        No staging profile named <span className="font-mono">{sandbox}</span> any
        more. Pick another one (or <span className="font-mono">off</span>) — a Run
        on a missing profile fails at launch, it does not fall back to a default.
      </span>
    )}
    {/* #452: what #410 used to do silently — clamp to `off` — stated instead of
        performed. Launch is blocked; the user demotes to `off` themselves or
        fixes Docker. */}
    {sandboxDoomed && (
      <span
        className="text-st-failed"
        style={{ fontSize: "10.5px" }}
        data-testid="sandbox-doomed-warning"
      >
        {sandbox === "" ? (
          <>
            The instance default is{" "}
            <span className="font-mono">{effectiveSandbox}</span>, which needs
            Docker
          </>
        ) : (
          <>
            <span className="font-mono">{effectiveSandbox}</span> needs Docker
          </>
        )}
        {" "}— the daemon reports it unavailable. Pick{" "}
        <span className="font-mono">off</span> to run on the host; this Run is not
        silently downgraded to it.
      </span>
    )}
    {inheritedDefaultReason && (
      <span
        className="text-st-failed"
        style={{ fontSize: "10.5px" }}
        data-testid="sandbox-default-reason"
      >
        {inheritedDefaultReason}
      </span>
    )}
    {dockerUnavailable && (
      <span
        className="text-fg-4"
        style={{ fontSize: "10.5px" }}
        data-testid="sandbox-docker-warning"
      >
        {sandboxReason}
      </span>
    )}
    {/* #452: a failed `GET /settings` leaves a one-entry list that reads like an
        instance without Docker. Say which one it is. Not blocking: inheriting is
        the safe answer here — the daemon resolves its own default. */}
    {settingsFailed && !settings && (
      <span
        className="text-fg-4"
        style={{ fontSize: "10.5px" }}
        data-testid="sandbox-settings-error"
      >
        Could not load instance settings, so the sandbox options are unavailable.
        This Run will use the instance default.
      </span>
    )}
  </div>
  );
}
