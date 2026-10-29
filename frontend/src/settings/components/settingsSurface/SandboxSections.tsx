/**
 * Settings › Sandbox & worktrees: the default sandbox (instance form), the staging profiles
 * and the instance's worktree provisioning (each saved at once).
 */

import type { SettingsSection } from "../../model/settingsCategories";
import { defaultSandboxSourceNote } from "../../model/sourceNotes";
import StagingProfilesPanel from "../../../sandbox/components/StagingProfilesPanel";
import { SettingRow } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import ProvisioningRows from "./ProvisioningRows";
import Section from "./Section";

export default function SandboxSections({
  sections,
  values,
  settings,
  dirty,
  setField,
  missingDefaultProfile,
  onProfilesChanged,
}: Omit<FormSectionProps, "section"> & {
  sections: SettingsSection[];
  missingDefaultProfile: boolean;
  onProfilesChanged: () => void;
}) {
  return (
    <>
      <Section section={sections[0]}>
        {/* Default sandbox (#410/#432): options are DATA — `off` plus the instance's
            staging profiles — and a stored name can dangle. A `PDO_DEFAULT_SANDBOX` naming
            a vanished profile is only visible here (the daemon's `reason`). */}
        <SettingRow
          label="Default sandbox"
          help="When neither the launch nor a Trigger picks one; off runs on the host, a staging profile in a Docker sandbox"
          source={defaultSandboxSourceNote(settings.default_sandbox)}
          keywords="PDO_DEFAULT_SANDBOX docker"
          dirty={dirty.has("default-sandbox")}
          htmlFor="setting-default-sandbox"
          testId="setting-row-default-sandbox"
          note={settings.default_sandbox.reason ?? undefined}
          noteTone="failed"
          noteTestId="setting-default-sandbox-reason"
        >
          <select
            id="setting-default-sandbox"
            data-testid="setting-default-sandbox"
            value={values.defaultSandbox}
            onChange={(e) => setField("defaultSandbox", e.target.value)}
            className="d-input d-input--mono set-ctl-w"
          >
            <option value="off">off (host)</option>
            {settings.sandbox_profiles.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
                {p.virtual ? " (built-in)" : ""}
              </option>
            ))}
            {/* Tombstone: a seeded value is NEVER silently rewritten. */}
            {missingDefaultProfile && (
              <option value={values.defaultSandbox} data-testid="setting-default-sandbox-missing">
                {values.defaultSandbox} (missing)
              </option>
            )}
          </select>
        </SettingRow>
      </Section>
      <StagingProfilesPanel
        section={sections[1]}
        home={settings.home ?? null}
        // Refetch `GET /settings` so the Default-sandbox select above sees the new name list
        // (and a freshly dangling `reason`) without a reopen, and tell New Run the same.
        onChanged={onProfilesChanged}
      />
      <ProvisioningRows section={sections[2]} />
    </>
  );
}
