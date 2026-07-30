import { Children, cloneElement, isValidElement } from "react";
import type { ReactElement, ReactNode } from "react";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";

export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <TooltipPrimitive.Provider delayDuration={300}>
      {children}
    </TooltipPrimitive.Provider>
  );
}

// #397: a Radix tooltip is a *description*, never a *name* — it wires
// `aria-describedby` on the trigger, and only while the tooltip is open. Every
// icon-only button wrapped in `<Tooltip>` was therefore left with an empty
// accessible name (WCAG 4.1.2): six anonymous `button` entries in the
// screen-reader element list, and nothing for voice control to say. The name is
// now supplied here so the next icon button is born named.
//
// The rule is deliberately narrow — the tooltip text is borrowed as a name only
// when the trigger cannot name itself. A text button (NodeInspector's
// `code-mutating` / `doc-only` pills) keeps its visible label: replacing it with
// a long tooltip sentence would break "label in name" (WCAG 2.5.3).
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

function withAccessibleName(children: ReactNode, content: string): ReactNode {
  if (!content.trim() || !isValidElement(children)) return children;

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

export function Tooltip({
  content,
  children,
  delay,
  side = "bottom",
}: {
  content: string;
  children: ReactNode;
  delay?: number;
  side?: "top" | "bottom" | "left" | "right";
}) {
  return (
    <TooltipPrimitive.Root delayDuration={delay}>
      <TooltipPrimitive.Trigger asChild>
        {withAccessibleName(children, content)}
      </TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          sideOffset={6}
          className="z-50 rounded border border-line bg-bg-4 px-2 py-1 text-fg shadow-lg"
          style={{ fontSize: "11px", maxWidth: 260, lineHeight: 1.4 }}
          data-testid="tooltip-content"
        >
          {content}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
