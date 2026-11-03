import type { SettingsSection } from "../../model/settingsCategories";
import AppearanceSection from "./AppearanceSection";
import InterfaceSection from "./InterfaceSection";
import ThemeSection from "./ThemeSection";

/**
 * Settings › Appearance (CONTEXT.md « Apparence »): the Theme, the Dark mode and the UI
 * scale, then one block per surface, then this browser's behaviour. Nothing here waits
 * for `GET /settings`, nor goes through the instance form's Save.
 */
export default function AppearanceCategory({ sections }: { sections: SettingsSection[] }) {
  return (
    <>
      <ThemeSection section={sections[0]} />
      <AppearanceSection section={sections[1]} />
      <InterfaceSection section={sections[2]} />
    </>
  );
}
