import { BarChart3, Settings } from "lucide-react";
import type { DecorationContext } from "../shared/extensions/contract";
import ExtensionBanner from "../shared/extensions/ExtensionBanner";
import ExtensionDecorations from "../shared/extensions/ExtensionDecorations";
import ExtensionNavigation from "../shared/extensions/ExtensionNavigation";
import ExtensionPageHost from "../shared/extensions/ExtensionPageHost";
import ExtensionTopbarEnd from "../shared/extensions/ExtensionTopbarEnd";
import { AcogniMark } from "../shared/ui/brand/AcogniMark";

/** What `topbar/account` adds to the base context: no item. */
const ACCOUNT_ITEM: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

/**
 * The app's top bar: the Acogni mark, the extensions' `topbar/start` entries, then
 * the right-hand group: the extensions' `topbar/end` icons, Stats, the
 * Settings gear, then the extensions' `topbar/account` components (an
 * avatar menu, say). Under it, the banner of the extensions whose interface could
 * not be loaded; over everything, the extension page open (by its entry or its
 * deep link).
 */
export default function TopBar({
  onOpenSettings,
  onOpenStats,
}: {
  onOpenSettings: () => void;
  onOpenStats: () => void;
}) {
  return (
    <>
      <header
        className="flex h-[44px] shrink-0 items-center gap-3 border-b border-line bg-surface-1 px-3 text-md"
      >
        <span className="grid shrink-0 place-items-center border-r border-line pr-3 text-text">
          <AcogniMark size={18} />
        </span>

        {/* ADR-0081: the extensions' `topbar/start` entries (none without extension). */}
        <ExtensionNavigation />

        {/* Right-aligned action group: the extensions' `topbar/end` icons (#29), stats
            (#377) then the settings gear (#129). */}
        <div className="ml-auto flex items-center gap-1">
          <ExtensionTopbarEnd />
          <button
            onClick={onOpenStats}
            aria-label="Open stats"
            data-testid="open-stats"
            className="grid h-6 w-6 place-items-center rounded text-text-3 transition-colors hover:bg-active hover:text-text"
          >
            <BarChart3 size={15} />
          </button>
          <button
            onClick={onOpenSettings}
            aria-label="Settings"
            data-testid="open-settings"
            className="grid h-6 w-6 place-items-center rounded text-text-3 transition-colors hover:bg-active hover:text-text"
          >
            <Settings size={15} />
          </button>
          {/* 0.8: the `topbar/account` components, after Settings (none without extension). */}
          <ExtensionDecorations<DecorationContext>
            location="topbar/account"
            extra={ACCOUNT_ITEM}
            className="ml-1 flex-nowrap"
          />
        </div>
      </header>
      {/* ADR-0081: a UI bundle built for another React is not loaded: the banner says so. */}
      <ExtensionBanner />
      <ExtensionPageHost />
    </>
  );
}
