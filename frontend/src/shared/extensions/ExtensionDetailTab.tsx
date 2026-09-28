import type { Attributes } from "./attributes";
import ExtensionBoundary from "./ExtensionBoundary";
import type { DetailTabProps, SlotElement } from "./slots";

/** The body of one extension tab: its component, given the entity. */
export default function ExtensionDetailTab({
  tab,
  entity,
}: {
  tab: SlotElement<DetailTabProps>;
  entity: { kind: string; id: string; attributes?: Attributes };
}) {
  const { Component, extension } = tab;
  return (
    <div className="px-3 py-3" style={{ fontSize: "11px" }} data-testid={`extension-tab-${tab.key}`}>
      <ExtensionBoundary extensionId={extension.id}>
        <Component
          extension={{ id: extension.id, name: extension.name }}
          entity={{ kind: entity.kind, id: entity.id, attributes: entity.attributes ?? {} }}
        />
      </ExtensionBoundary>
    </div>
  );
}
