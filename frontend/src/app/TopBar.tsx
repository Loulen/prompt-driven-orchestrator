import { BarChart3, Settings } from "lucide-react";
import ExtensionBanner from "../shared/extensions/ExtensionBanner";
import ExtensionNavigation from "../shared/extensions/ExtensionNavigation";

/**
 * The app's top bar: the PDO mark, the extensions' navigation entries, then
 * Stats and the Settings gear. Under it, the banner of the extensions whose
 * interface could not be loaded.
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

        {/* ADR-0081: the extensions' navigation entries (none without extension). */}
        <ExtensionNavigation />

        {/* Right-aligned action group: stats (#377) then the settings gear (#129). */}
        <div className="ml-auto flex items-center gap-1">
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
        </div>
      </header>
      {/* ADR-0081: a UI bundle built for another React is not loaded: the banner says so. */}
      <ExtensionBanner />
    </>
  );
}
