import type { ReactNode } from "react";
import { Button } from "../ui/button";
import ExtensionBoundary from "./ExtensionBoundary";
import ExtensionIcon from "./ExtensionIcon";
import { runHostedCommand, sourceOf, useHostedContributions, type HostContext, type HostedElement } from "./hosted";

/**
 * One element: its component (with `extension`, `context` and `props`), inside
 * an error boundary; for an action, a button with its command's icon and title
 * that calls it.
 */
export function HostedContribution({
  element,
  props,
}: {
  element: HostedElement;
  props?: Record<string, unknown>;
}): ReactNode {
  const source = sourceOf(element);
  if (!source) return null;
  const { Component, command } = source;
  if (command) {
    return (
      <ExtensionBoundary extensionId={element.extension.id}>
        <Button
          size="xs"
          variant="outline"
          onClick={() => void runHostedCommand(element)}
          data-testid={`hosted-action-${element.key}`}
        >
          <ExtensionIcon url={command.icon} label={element.label} size={12} />
          {element.label}
        </Button>
      </ExtensionBoundary>
    );
  }
  if (!Component) return null;
  const Rendered = Component as (p: Record<string, unknown>) => ReactNode;
  return (
    <ExtensionBoundary extensionId={element.extension.id}>
      <Rendered {...props} extension={element.extension} context={element.context} />
    </ExtensionBoundary>
  );
}

/**
 * Every element of the hosted location `location`, in order, each through
 * `HostedContribution` and wrapped by `wrap` when given. Nothing at all when
 * none: an empty location takes no room.
 */
export function HostedContributions({
  location,
  context,
  props,
  wrap,
}: {
  location: string;
  context?: HostContext;
  props?: Record<string, unknown>;
  wrap?: (element: HostedElement, rendered: ReactNode) => ReactNode;
}): ReactNode {
  const elements = useHostedContributions(location, context);
  if (elements.length === 0) return null;
  return (
    <>
      {elements.map((element) => {
        const rendered = <HostedContribution element={element} props={props} />;
        return (
          <HostedSlot key={element.key} testId={`hosted-${element.key}`}>
            {wrap ? wrap(element, rendered) : rendered}
          </HostedSlot>
        );
      })}
    </>
  );
}

/** A keyed fragment with a test hook, adding no box to the host's layout. */
function HostedSlot({ children, testId }: { children: ReactNode; testId: string }) {
  return (
    <span data-testid={testId} style={{ display: "contents" }}>
      {children}
    </span>
  );
}
