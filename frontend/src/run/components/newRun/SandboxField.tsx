import { Box, ChevronDown, Settings2 } from "lucide-react";
import { IconButton } from "../../../shared/ui/kit";
import FormRow from "../../../shared/ui/FormRow";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/**
 * The Sandbox row (#410/#432/#452): a native select in the kit's dropdown
 * look, whose options are data (`GET /settings`); the reason a choice cannot
 * launch at the row's end, in a word, its sentence in the tooltip.
 */
export default function SandboxField({ form, onManageStagingProfiles }: { form: NewRunForm; onManageStagingProfiles?: () => void }) {
  const {
    mode, settings, settingsFailed, sandbox, setSandbox, dockerUnavailable, sandboxReason,
    sandboxProfiles, missingProfile, instanceDefaultSandbox, effectiveSandbox, sandboxDoomed,
    inheritedDefaultReason,
  } = form;
  return (
    <FormRow label="Sandbox" htmlFor="sandbox-select">
      <span className="ln-native ln-grow" data-invalid={missingProfile || sandboxDoomed || !!inheritedDefaultReason || undefined}>
        <Box size={13} strokeWidth={1.5} className="ln-native__lead" aria-hidden />
        <select
          id="sandbox-select"
          data-testid="sandbox-select"
          value={sandbox}
          onChange={(e) => setSandbox(e.target.value)}
          title={dockerUnavailable ? sandboxReason : undefined}
          className="ln-native__el"
        >
          {/* #452: run mode names the resolved default, because the choice is made
              NOW. A Trigger resolves when it fires, so naming today's value there
              would be a promise the modal cannot keep. */}
          <option value="">
            {mode === "run" && instanceDefaultSandbox
              ? `Default (${instanceDefaultSandbox})`
              : "Default"}
          </option>
          <option value="off">Off, on the host</option>
          {sandboxProfiles.map((p) => (
            <option key={p.name} value={p.name} disabled={dockerUnavailable}>
              {dockerUnavailable ? `${p.name} (Docker unavailable)` : p.name}
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
        <ChevronDown size={12} strokeWidth={1.5} className="ln-native__chev" aria-hidden />
      </span>
      {missingProfile && (
        <span
          className="ln-err"
          title={`No staging profile named ${sandbox}: a Run on it fails at launch, it does not fall back to a default.`}
          data-testid="sandbox-missing-profile-warning"
        >
          Missing profile
        </span>
      )}
      {/* #452: what #410 used to do silently — clamp to `off` — stated instead of
          performed: launch is blocked until `off` is chosen or Docker is back. */}
      {sandboxDoomed && (
        <span
          className="ln-err"
          title={`${sandbox === "" ? `The instance default, ${effectiveSandbox},` : effectiveSandbox} needs Docker, which the daemon reports unavailable. Pick off to run on the host.`}
          data-testid="sandbox-doomed-warning"
        >
          Needs Docker
        </span>
      )}
      {inheritedDefaultReason && (
        <span className="ln-err" title={inheritedDefaultReason} data-testid="sandbox-default-reason">
          Default unavailable
        </span>
      )}
      {dockerUnavailable && !sandboxDoomed && !missingProfile && (
        <span className="ln-faint ln-rowend" title={sandboxReason} data-testid="sandbox-docker-warning">
          No Docker
        </span>
      )}
      {/* #452: a failed `GET /settings` leaves a one-entry list that reads like an
          instance without Docker. Not blocking: the daemon resolves its default. */}
      {settingsFailed && !settings && (
        <span className="ln-faint ln-rowend" title="Instance settings could not load: the Run uses the instance default." data-testid="sandbox-settings-error">
          Settings unavailable
        </span>
      )}
      {onManageStagingProfiles && (
        <IconButton
          icon={Settings2}
          label="Manage staging profiles"
          onClick={onManageStagingProfiles}
          data-testid="new-run-manage-staging-profiles"
        />
      )}
    </FormRow>
  );
}
