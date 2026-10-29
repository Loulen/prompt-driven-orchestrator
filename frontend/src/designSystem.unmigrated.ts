/**
 * The source files not migrated to the shared design system yet (ADR-0091):
 * each still sets a font size in pixels (an inline `fontSize`, a
 * `text-[Npx]`, a CSS `font-size: Npx`) instead of the type scale, or a hex
 * colour instead of a token. `designSystem.guards.test.ts` holds the rest of
 * the source to the rule.
 *
 * The list only shrinks: a change that migrates a file removes it here (the
 * guard fails while a listed file is clean), and no file joins it. It must be
 * empty before the design system's story ships.
 */
export const UNMIGRATED: readonly string[] = [
  "app/tour/TutorialsSection.tsx",
  "harness/components/EffortPicker.tsx",
  "harness/components/HarnessSelect.tsx",
  "harness/components/ModelPicker.tsx",
  "platform/components/VersionUpdateSection.tsx",
  "sandbox/components/DeleteProfileDialog.tsx",
  "sandbox/components/ProfileEntries.tsx",
  "sandbox/components/ProfileEnvEditor.tsx",
  "sandbox/components/ProfileImageEditor.tsx",
  "sandbox/components/StagingProfilesPanel.tsx",
  "settings/components/AgentProfileModal.tsx",
  "settings/components/AgentProfilesPanel.tsx",
  "settings/components/SettingsSurface.tsx",
  "settings/components/settingsSurface/AgentsSections.tsx",
  "settings/components/settingsSurface/CategoryPage.tsx",
  "settings/components/settingsSurface/ClonesFolderSection.tsx",
  "settings/components/settingsSurface/DiagnosticsRows.tsx",
  "settings/components/settingsSurface/GeneralSections.tsx",
  "settings/components/settingsSurface/PublicUrlSection.tsx",
  "settings/components/settingsSurface/SandboxSections.tsx",
  "settings/components/settingsSurface/Section.tsx",
  "settings/components/settingsSurface/SettingsFooter.tsx",
  "settings/components/settingsSurface/TelemetryMetricsIdentity.tsx",
  "settings/components/settingsSurface/TelemetrySections.tsx",
  "settings/components/settingsSurface/fields.tsx",
  "settings/components/settingsSurface/overlays.tsx",
  "shared/extensions/ExtensionSettingsPanels.tsx",
  "shared/ui/ThemeSelect.tsx",
  "shared/ui/key-value-row.tsx",
  "workspace/components/ProvisioningResolvedPlan.tsx",
  "workspace/components/ProvisioningRulesEditor.tsx",
  "workspace/components/projects/AddRepositoryMenu.tsx",
  "workspace/components/projects/NewRepositoryForm.tsx",
  "workspace/components/projects/ProjectDetail.tsx",
  "workspace/components/projects/ProjectList.tsx",
  "workspace/components/projects/ProjectsPanel.tsx",
];
