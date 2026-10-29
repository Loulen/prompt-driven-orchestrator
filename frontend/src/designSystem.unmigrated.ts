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
  "workspace/components/ProvisioningRulesEditor.tsx",
];
