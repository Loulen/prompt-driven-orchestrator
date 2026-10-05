import { BarChart3, Settings } from "lucide-react";
import type { DecorationContext } from "../shared/extensions/contract";
import ExtensionBanner from "../shared/extensions/ExtensionBanner";
import ExtensionDecorations from "../shared/extensions/ExtensionDecorations";
import ExtensionNavigation from "../shared/extensions/ExtensionNavigation";
import ExtensionPageHost from "../shared/extensions/ExtensionPageHost";
import ExtensionTopbarEnd from "../shared/extensions/ExtensionTopbarEnd";

/** What `topbar/account` adds to the base context: no item. */
const ACCOUNT_ITEM: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

/**
 * The app's top bar: the PDO mark, the extensions' `topbar/start` entries, then
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
        className="flex h-[44px] shrink-0 items-center gap-3 border-b border-line bg-bg-2 px-3"
        style={{ fontSize: "12.5px" }}
      >
        <div className="flex items-center gap-2 border-r border-line pr-3 font-semibold tracking-tight text-fg">
          <span className="grid h-[18px] w-[18px] place-items-center text-acc">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path
                d="M7 1L12.5 4.5V9.5L7 13L1.5 9.5V4.5L7 1Z"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinejoin="round"
              />
              <circle cx="7" cy="7" r="2" fill="currentColor" />
            </svg>
          </span>
          PDO
        </div>

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
            className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
          >
            <BarChart3 size={15} />
          </button>
          <button
            onClick={onOpenSettings}
            aria-label="Settings"
            data-testid="open-settings"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
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
