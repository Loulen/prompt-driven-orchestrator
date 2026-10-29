import ExtensionBoundary from "./ExtensionBoundary";
import type { ContributionElement } from "./catalogue";
import type { TabContext, TabProps } from "./contract";

/** The body of one extension tab of `run/detail`: its component, given the entity. */
export default function ExtensionDetailTab({
  tab,
  context,
}: {
  tab: ContributionElement<TabProps>;
  context: TabContext;
}) {
  const { Component, extension } = tab;
  if (!Component) return null;
  return (
    <div className="pi-ext" data-testid={`extension-tab-${tab.key}`}>
      <ExtensionBoundary extensionId={extension.id}>
        <Component
          extension={{ id: extension.id, name: extension.name }}
          context={context}
          entity={context.entity}
        />
      </ExtensionBoundary>
    </div>
  );
}
