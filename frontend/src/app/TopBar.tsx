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
import { canCreateSomeRun, NO_RUN_ANYWHERE } from "../run/model/runCreationRights";
import { useRunCreationRightsStore } from "../run/store/runCreationRightsStore";
import CommandField from "./palette/CommandField";
import CommandPalette from "./palette/CommandPalette";

/** What `topbar/account` adds to the base context: no item. */
const ACCOUNT_ITEM: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

/**
 * The app's top bar, 36 px: the Acogni mark, the extensions' `topbar/start`
 * entries, the command field in the middle, then the right-hand group: the
 * extensions' `topbar/end` icons, New run, Stats, Settings, the theme, and the
 * extensions' `topbar/account` components (an avatar menu, say). Under it, the
 * banner of the extensions whose interface could not be loaded; over
 * everything, the extension page open (by its entry or its deep link) and the
 * command palette.
 */
export default function TopBar({
  onOpenSettings,
  onOpenStats,
  onNewRun,
}: {
  onOpenSettings: () => void;
  onOpenStats: () => void;
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
          <IconButton icon={StatsIcon} label="Stats" onClick={onOpenStats} data-testid="open-stats" />
          <IconButton icon={SettingsIcon} label="Settings" onClick={onOpenSettings} data-testid="open-settings" />
          <ThemeToggle />
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

/** New run, disabled with its reason when no Projet allows creating a Run (CORE-5). */
function NewRunButton({ onNewRun }: { onNewRun: () => void }) {
  const canCreate = useRunCreationRightsStore((s) => canCreateSomeRun(s.rights));
  return (
    <IconButton
      icon={Plus}
      label="New run"
      description={canCreate ? undefined : NO_RUN_ANYWHERE}
      aria-disabled={canCreate ? undefined : true}
      onClick={canCreate ? onNewRun : undefined}
      data-testid="topbar-new-run"
    />
  );
}

/** Switches between the dark and the light theme (Settings › Interface also offers System). */
function ThemeToggle() {
  const { resolved, setPreference } = useTheme();
  const dark = resolved === "dark";
  return (
    <IconButton
      icon={dark ? Sun : Moon}
      label={dark ? "Light theme" : "Dark theme"}
      onClick={() => setPreference(dark ? "light" : "dark")}
      data-testid="theme-toggle"
    />
  );
}
