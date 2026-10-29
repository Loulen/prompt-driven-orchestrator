import type { SettingsRailEntry } from "./SettingsRail";

/** The core's categories, then the extensions' entries, the first of them under a rule. */
export function settingsRail(core: SettingsRailEntry[], extensions: SettingsRailEntry[]): SettingsRailEntry[] {
  return [...core, ...extensions.map((entry, index) => (index === 0 ? { ...entry, group: true } : entry))];
}
