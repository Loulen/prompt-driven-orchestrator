import { Moon, Plus, Sun } from "lucide-react";
import type { DecorationContext } from "../shared/extensions/contract";
import ExtensionBanner from "../shared/extensions/ExtensionBanner";
import ExtensionDecorations from "../shared/extensions/ExtensionDecorations";
import ExtensionNavigation from "../shared/extensions/ExtensionNavigation";
import ExtensionPageHost from "../shared/extensions/ExtensionPageHost";
import ExtensionTopbarEnd from "../shared/extensions/ExtensionTopbarEnd";
import { AcogniMark } from "../shared/ui/brand/AcogniMark";
import { SettingsIcon, StatsIcon } from "../shared/ui/brand/AccentIcons";
import { IconButton } from "../shared/ui/kit";
import { useTheme } from "../hooks/useTheme";
import { whyNoRunAnywhere } from "../run/model/runCreationRights";
import { useRunCreationRightsStore } from "../run/store/runCreationRightsStore";
import CommandField from "./palette/CommandField";
import CommandPalette from "./palette/CommandPalette";

/** What `topbar/account` adds to the base context: no item. */
const ACCOUNT_ITEM: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

/**
 * The app's top bar, 36 px: the Acogni mark, the extensions' `topbar/start`
 * entries, the command field in the middle, then the right-hand group: the
 * extensions' `topbar/end` icons, New run, Stats, Settings, the Dark mode, and the
 * extensions' `topbar/account` components (an avatar menu, say). Under it, the
 * banner of the extensions whose interface could not be loaded, and the
 * extension page open (by its entry or its deep link), under the bar like
 * Settings and Stats (CORE-183); over everything, the command palette.
 *
 * The entry of the surface open under the bar is selected (`aria-pressed`): a
 * second click on it closes the surface, another entry replaces it.
 */
export default function TopBar({
  onOpenSettings,
  onOpenStats,
  settingsOpen = false,
  statsOpen = false,
  onNewRun,
}: {
  /** Settings' icon: opens Settings, or closes it when it is open. */
  onOpenSettings: () => void;
  /** Stats' icon: opens Stats, or closes it when it is open. */
  onOpenStats: () => void;
  settingsOpen?: boolean;
  statsOpen?: boolean;
  /** Absent: no New run in the bar. */
  onNewRun?: () => void;
}) {
  return (
    <>
      <header className="sh-tb" data-testid="topbar">
        <span className="sh-tb__mark">
          <AcogniMark size={16} />
        </span>

        {/* ADR-0081: the extensions' `topbar/start` entries (none without extension). */}
        <ExtensionNavigation />

        <div className="sh-tb__mid">
          <CommandField />
        </div>

        <div className="sh-tb__end">
          {/* The extensions' `topbar/end` icons (#29), before the core's. */}
          <ExtensionTopbarEnd />
          {onNewRun && <NewRunButton onNewRun={onNewRun} />}
          <IconButton
            icon={StatsIcon}
            label="Stats"
            pressed={statsOpen}
            className="sh-tb__surface"
            onClick={onOpenStats}
            data-testid="open-stats"
          />
          <IconButton
            icon={SettingsIcon}
            label="Settings"
            pressed={settingsOpen}
            className="sh-tb__surface"
            onClick={onOpenSettings}
            data-testid="open-settings"
          />
          <DarkModeToggle />
          {/* 0.8: the `topbar/account` components, at the far right (none without extension). */}
          <ExtensionDecorations<DecorationContext>
            location="topbar/account"
            extra={ACCOUNT_ITEM}
            className="sh-tb__account flex-nowrap"
          />
        </div>
      </header>
      {/* ADR-0081: a UI bundle built for another React is not loaded: the banner says so. */}
      <ExtensionBanner />
      <ExtensionPageHost />
      <CommandPalette />
    </>
  );
}

/** New run, greyed with its reason when no Projet allows creating a Run (CORE-5, the kit's `disabledReason`: CORE-205). */
function NewRunButton({ onNewRun }: { onNewRun: () => void }) {
  const noRunReason = useRunCreationRightsStore((s) => whyNoRunAnywhere(s.rights));
  return (
    <IconButton
      icon={Plus}
      label="New run"
      disabledReason={noRunReason}
      onClick={onNewRun}
      data-testid="topbar-new-run"
    />
  );
}

/** Switches the Dark mode (ADR-0093); the Theme is chosen in Settings › Interface. */
function DarkModeToggle() {
  const { mode, toggleMode } = useTheme();
  const dark = mode === "dark";
  return (
    <IconButton
      icon={dark ? Sun : Moon}
      label={dark ? "Dark mode off" : "Dark mode on"}
      // A toggle: its state on `aria-pressed`, under a name that does not move.
      aria-label="Dark mode"
      pressed={dark}
      className="sh-tb__mode"
      onClick={toggleMode}
      data-testid="dark-mode-toggle"
    />
  );
}
