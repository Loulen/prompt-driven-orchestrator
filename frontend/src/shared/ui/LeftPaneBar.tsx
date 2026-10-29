import { useContext, type ReactNode } from "react";
import { LeftTabsContext } from "./leftTabs";
import { Tooltip } from "./kit";

/**
 * The 32 px header of a pane of the left panel: the strip of tab icons, the
 * pane's title in capitals, then its actions (icon buttons) on the right.
 * Outside the left panel (a pane rendered alone), the title and the actions.
 */
export default function LeftPaneBar({
  title,
  children,
  testId,
}: {
  title: string;
  /** The pane's actions. */
  children?: ReactNode;
  testId?: string;
}) {
  const tabs = useContext(LeftTabsContext);
  return (
    <div className="d-bar sh-left-head" data-testid={testId}>
      {tabs && (
        <div className="sh-left-head__tabs" role="tablist" aria-label="Left panel">
          {tabs.tabs.map((tab) => {
            const on = tab.id === tabs.active;
            const TabIcon = tab.icon;
            return (
              <Tooltip key={tab.id} label={tab.label}>
                <button
                  type="button"
                  role="tab"
                  aria-selected={on}
                  aria-label={tab.label}
                  // #823: a tour has to be able to aim at a tab — its first step opens
                  // the one the rest of the tour lives in.
                  data-testid={`left-tab-${tab.id}`}
                  className="d-icon sh-left-head__tab"
                  onClick={() => tabs.onChange(tab.id)}
                >
                  <TabIcon size={14} strokeWidth={1.5} aria-hidden />
                  {/* #577 — a dot left on a NON-active tab with a live selection. */}
                  {!on && tab.selected > 0 && (
                    <span
                      className="sh-left-head__dot"
                      data-testid={`tab-badge-${tab.id}`}
                      aria-label={`${tab.selected} selected`}
                    />
                  )}
                </button>
              </Tooltip>
            );
          })}
        </div>
      )}
      <span className="d-caps sh-left-head__title" title={title}>
        {title}
      </span>
      {children && <div className="sh-left-head__acts">{children}</div>}
    </div>
  );
}
