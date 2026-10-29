import { Children, cloneElement, isValidElement } from "react";
import type { ReactElement, ReactNode } from "react";
import { TooltipLayer } from "./kit/tip";

// #397: a tooltip is a *description*, never a *name*. Every
// icon-only button wrapped in `<Tooltip>` was therefore left with an empty
// accessible name (WCAG 4.1.2): six anonymous `button` entries in the
// screen-reader element list, and nothing for voice control to say. The name is
// now supplied here so the next icon button is born named.
//
// The rule is deliberately narrow — the tooltip text is borrowed as a name only
// when the trigger cannot name itself. A text button (NodeInspector's Workspace
// choices) keeps its visible label: replacing it with a long tooltip sentence
// would break "label in name" (WCAG 2.5.3).
const INTERACTIVE_TAGS = new Set(["button", "a"]);

/** Text this element renders itself. Host elements are transparent (recurse); a
 *  custom component's output is unknowable here, so it doesn't count. */
function rendersText(node: ReactNode): boolean {
  return Children.toArray(node).some((child) => {
    if (typeof child === "string") return child.trim() !== "";
    if (typeof child === "number") return true;
    if (isValidElement(child) && typeof child.type === "string") {
      return rendersText((child.props as { children?: ReactNode }).children);
    }
    return false;
  });
}

function withAccessibleName(children: ReactNode, content: ReactNode): ReactNode {
  // Only a plain-text tooltip can lend a name; rich content (a node's duration
  // breakdown) sits on triggers that name themselves.
  if (typeof content !== "string" || !content.trim() || !isValidElement(children)) {
    return children;
  }

  const el = children as ReactElement<Record<string, unknown>>;
  const props = el.props;

  // An `aria-label` on a bare div/span is inert (no role, no name computed), so
  // don't emit one — only intrinsic interactive tags and explicit roles.
  const interactive =
    typeof el.type === "string" &&
    (INTERACTIVE_TAGS.has(el.type) || props.role != null);
  if (!interactive) return children;

  // Anything the trigger declares itself wins.
  if (
    props["aria-label"] != null ||
    props["aria-labelledby"] != null ||
    props.title != null ||
    rendersText(props.children as ReactNode)
  ) {
    return children;
  }

  return cloneElement(el, { "aria-label": content });
}

/**
 * The core's tooltip, on the shared design system's layer (`kit/primitives`):
 * the bubble of `kit.tooltip`, placed under the UI scale. `content` is the
 * name and shortcut as text ("Undo (Ctrl+Z)"), or for a structured tooltip a
 * node.
 */
export function Tooltip({
  content,
  children,
  delay,
  side = "bottom",
  disabled,
}: {
  /** Plain text (multi-line via `\n`) or, for a structured tooltip, a node. */
  content: ReactNode;
  children: ReactNode;
  delay?: number;
  side?: "top" | "bottom" | "left" | "right";
  /** Keep the trigger mounted but never open. For a hint that only applies in
   *  some states (#867's read-only hint): unwrapping the child instead would
   *  remount it. */
  disabled?: boolean;
}) {
  return (
    <TooltipLayer content={content} side={side} delay={delay} disabled={disabled}>
      {withAccessibleName(children, content)}
    </TooltipLayer>
  );
}
