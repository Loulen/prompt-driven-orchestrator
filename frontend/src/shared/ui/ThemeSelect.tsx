/* Theme selector (#759): system · light · dark, device-local, written at the change.
   Lives in Settings › General › Interface next to Single-tab mode — same nature
   (a presentation preference this browser owns, never `instance_config`).

   `System` names, in its tooltip, the theme it WOULD resolve to (the OS
   preference): "System" alone tells you nothing about what you are about to
   get. Not the painted theme: with Dark pinned on a light OS, "System (dark)"
   would promise the opposite of what clicking it does.

   The kit's segmented control: one tab stop, arrows move and select. */

import { Segmented } from "./kit";
import { useTheme } from "../../hooks/useTheme";
import type { ThemePreference } from "../../lib/theme";

export default function ThemeSelect() {
  const { preference, systemResolved, setPreference } = useTheme();
  return (
    <span data-testid="theme-select" className="contents">
      <Segmented<ThemePreference>
        label="Theme"
        size="sm"
        value={preference}
        onChange={setPreference}
        items={[
          { value: "system", label: "System", title: `System (${systemResolved})` },
          { value: "light", label: "Light" },
          { value: "dark", label: "Dark" },
        ]}
      />
    </span>
  );
}
