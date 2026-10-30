/* Theme selector (ADR-0093): Acogni · Classic, device-local, written at the
   change. Lives in Settings › General › Interface, above the Dark mode switch:
   a Theme never names dark or light, the mode is the next row.

   The kit's segmented control: one tab stop, arrows move and select. */

import { Segmented } from "./kit";
import { useTheme } from "../../hooks/useTheme";
import type { Theme } from "../../lib/theme";

export default function ThemeSelect() {
  const { theme, setTheme } = useTheme();
  return (
    <span data-testid="theme-select" className="contents">
      <Segmented<Theme>
        label="Theme"
        size="sm"
        value={theme}
        onChange={setTheme}
        items={[
          { value: "acogni", label: "Acogni" },
          { value: "classic", label: "Classic" },
        ]}
      />
    </span>
  );
}
