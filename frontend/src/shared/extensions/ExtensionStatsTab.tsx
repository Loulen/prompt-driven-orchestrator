import ExtensionBoundary from "./ExtensionBoundary";
import type { ContributionElement } from "./catalogue";
import type { StatsTabContext, StatsTabProps } from "./contract";

/**
 * The body of one extension tab of `stats/tab`: its component, in the Stats
 * frame (the header's period and Refresh stay the core's), behind its error
 * boundary.
 */
export default function ExtensionStatsTab({
  tab,
  context,
}: {
  tab: ContributionElement<StatsTabProps>;
  context: StatsTabContext;
}) {
  const { Component, extension } = tab;
  if (!Component) return null;
  return (
    <div className="flex min-w-0 flex-col gap-4" data-testid={`stats-extension-tab-${tab.key}`}>
      <ExtensionBoundary extensionId={extension.id}>
        <Component extension={{ id: extension.id, name: extension.name }} context={context} />
      </ExtensionBoundary>
    </div>
  );
}
