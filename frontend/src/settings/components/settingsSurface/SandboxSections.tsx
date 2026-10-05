/**
 * Settings › Sandbox & worktrees: the default sandbox (instance form), the staging profiles
 * and the instance's worktree provisioning (each with its own persistence).
 */

import type { SettingsSection } from "../../model/settingsCategories";
import { defaultSandboxSourceNote } from "../../model/sourceNotes";
import StagingProfilesPanel from "../../../sandbox/components/StagingProfilesPanel";
import PersistedProvisioningEditor from "../../../workspace/components/PersistedProvisioningEditor";
import { FieldBlock } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
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
      {/* Default sandbox (#410/#432): options are DATA — `off` plus the
          instance's staging profiles — and a stored name can dangle. */}
      <FieldBlock
        label="Default sandbox"
        htmlFor="setting-default-sandbox"
        dirty={dirty.has("default-sandbox")}
        help={
          <>
            What a Run uses when neither the launch dialog nor a firing Trigger
            picks one. <span className="font-mono">off</span> runs on the host; a{" "}
            <span className="font-mono">staging profile</span> runs it inside a
            Docker sandbox with that profile's home content (requires Docker).
          </>
        }
        source={defaultSandboxSourceNote(settings.default_sandbox)}
        sourceTestId="setting-source-default-sandbox"
      >
        <select
          id="setting-default-sandbox"
          data-testid="setting-default-sandbox"
          value={values.defaultSandbox}
          onChange={(e) => setField("defaultSandbox", e.target.value)}
          className={`w-full rounded-md border bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none ${
            dirty.has("default-sandbox") ? "border-st-await" : "border-line-strong"
          }`}
          style={{ fontSize: "12px" }}
        >
          <option value="off">off (run on the host)</option>
          {settings.sandbox_profiles.map((p) => (
            <option key={p.name} value={p.name}>
              {p.name}
              {p.virtual ? " (built-in)" : ""}
            </option>
          ))}
          {/* Tombstone: a seeded value is NEVER silently rewritten. */}
          {missingDefaultProfile && (
            <option
              value={values.defaultSandbox}
              data-testid="setting-default-sandbox-missing"
            >
              {values.defaultSandbox} — missing
            </option>
          )}
        </select>
        {/* Server-supplied: a `PDO_DEFAULT_SANDBOX` naming a vanished profile is
            only visible here. */}
        {settings.default_sandbox.reason && (
          <div
            className="text-st-failed"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-default-sandbox-reason"
          >
            {settings.default_sandbox.reason}
          </div>
        )}
      </FieldBlock>
    </Section>
    <Section section={sections[1]}>
      {/* Inline (#691), natural height: the page scrolls, not the panel. */}
      <div className="rounded-md border border-line bg-bg-3/40">
        <StagingProfilesPanel
          home={settings.home ?? null}
          // Refetch `GET /settings` so the Default-sandbox `<select>` above sees the
          // new name list (and a freshly dangling `reason`) without a reopen, and
          // tell New Run (mounted underneath) the same thing.
          onChanged={onProfilesChanged}
        />
      </div>
    </Section>
    <Section section={sections[2]}>
      {/* The instance-scope editor IS the section; its own Save provisioning stays. */}
      <PersistedProvisioningEditor scope="instance" defaultExpanded />
    </Section>
    </>
  );
}
