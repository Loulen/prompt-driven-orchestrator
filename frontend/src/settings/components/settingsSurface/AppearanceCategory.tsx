import type { SettingsSection } from "../../model/settingsCategories";
import AppearanceSection from "./AppearanceSection";
import InterfaceSection from "./InterfaceSection";
import ThemeSection from "./ThemeSection";
import type { AppearanceEntities } from "./surfaceProps";

/**
 * Settings › Appearance (CONTEXT.md « Apparence »): the Theme, the Dark mode and the UI
 * scale, then one block per surface, then this browser's behaviour. Nothing here waits
 * for `GET /settings`, nor goes through the instance form's Save.
 */
export default function AppearanceCategory({
  sections,
  entities,
}: {
  sections: SettingsSection[];
  entities?: AppearanceEntities;
}) {
  return (
    <>
      <ThemeSection section={sections[0]} />
      <AppearanceSection section={sections[1]} entities={entities} />
      <InterfaceSection section={sections[2]} />
    </>
  );
}
