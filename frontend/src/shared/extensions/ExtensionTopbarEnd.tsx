import { useCallback, useEffect, useId, useRef, useState, type ComponentType } from "react";
import { Tooltip } from "../ui/kit";
import { AnchoredPanel, FOCUSABLE } from "../ui/kit/layer";
import { useContributions, useLocationContext, type ContributionElement } from "./catalogue";
import { runCommand } from "./commands";
import type { LocationContext, PanelProps } from "./contract";
import { isPageOpen, togglePage, usePages } from "./deepLink";
import ExtensionBoundary from "./ExtensionBoundary";
import ExtensionIcon from "./ExtensionIcon";
import { usePageCounter } from "./pageCounter";

/**
 * The `topbar/end` location: one icon button per element, in the top bar's
 * right-hand group, before the core's icons, a hairline after them. A page's
 * button opens it full-window, under the bar, and stays selected while it is
 * on screen (`aria-pressed`, CORE-183): a second click closes it. A panel's
 * opens it under the icon (0.20); an
 * action's calls its command with the location's context. An element that
 * declares a counter (a page 0.19, a panel or an action 0.20) shows it on its
 * icon, for the current Actor, live. Nothing without a contribution.
 */
export default function ExtensionTopbarEnd() {
  const context = useLocationContext("topbar/end");
  const elements = useContributions("topbar/end", context);
  const open = usePages((s) => s.open);
  if (elements.length === 0) return null;
  return (
    <>
    {/* The icons shrink out of sight first, the hairline after them stays. */}
    <div className="sh-tb__ext" role="group" aria-label="Extensions" data-testid="extension-topbar-end">
      {elements.map((element) => (
        <ExtensionBoundary key={element.key} extensionId={element.extension.id}>
          {element.contribution.type === "panel" ? (
            <TopbarEndPanel element={element} context={context} />
          ) : (
            <TopbarEndButton
              element={element}
              pressed={
                element.contribution.type === "page" ? isPageOpen(open, "topbar/end", element.key) : undefined
              }
              onClick={() => activate(element, context)}
            />
          )}
        </ExtensionBoundary>
      ))}
    </div>
    <span className="sh-tb__sep" aria-hidden />
    </>
  );
}

function activate(element: ContributionElement, context: LocationContext) {
  const { contribution, extension } = element;
  if (contribution.type === "page") {
    togglePage({ key: element.key, extension: extension.id, location: "topbar/end", path: "" });
  } else {
    void runCommand(element, context);
  }
}

/** The icon of an element: a page's or a panel's own, an action's command's. */
function iconOf({ contribution, command }: ContributionElement): string | null | undefined {
  return contribution.type === "page" || contribution.type === "panel" ? contribution.icon : command?.icon;
}

function TopbarEndButton({
  element,
  onClick,
  open,
  pressed,
}: {
  element: ContributionElement;
  onClick: () => void;
  /** A panel's button: whether its panel is open. */
  open?: boolean;
  /** A page's button: whether its page is on screen. */
  pressed?: boolean;
}) {
  const { contribution, extension } = element;
  const label = contribution.label;
  const counter = usePageCounter(extension.id, contribution.counter);
  const count = counter && counter.count > 0 ? counter.count : null;
  return (
    <Tooltip label={label} description={count ? (counter?.detail ?? undefined) : undefined}>
      <button
        type="button"
        onClick={onClick}
        aria-label={count ? `${label}, ${count}` : label}
        aria-expanded={open}
        aria-haspopup={open === undefined ? undefined : "dialog"}
        aria-pressed={pressed}
        data-testid={`extension-topbar-end-${element.key}`}
        className={pressed === undefined ? "d-icon" : "d-icon sh-tb__surface"}
      >
        <ExtensionIcon url={iconOf(element)} label={label} />
        {count && (
          <span
            className="d-icon__badge sh-tb__count"
            data-accent={counter?.accent || undefined}
            data-testid={`extension-topbar-end-count-${element.key}`}
            aria-hidden
          >
            {count > 99 ? "99+" : count}
          </span>
        )}
      </button>
    </Tooltip>
  );
}

/**
 * A panel's icon and the panel it opens under it, right-aligned on the icon
 * (0.20). Escape, a click outside, a scroll or a resize close it; a click in
 * a toast does not (its « Undo » acts on what the panel did); the icon toggles
 * it. Opening focuses its first control, or the panel itself while it has none
 * (its content still loading); Escape returns the focus to the icon.
 */
function TopbarEndPanel({ element, context }: { element: ContributionElement; context: LocationContext }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLSpanElement>(null);
  const id = useId();
  const close = useCallback(() => setOpen(false), []);
  useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() => {
      const panel = document.getElementById(id);
      if (!panel) return;
      const first = panel.querySelector<HTMLElement>(FOCUSABLE);
      if (!first) panel.tabIndex = -1;
      (first ?? panel).focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [open, id]);
  const { extension, contribution } = element;
  const Component = element.Component as ComponentType<PanelProps> | null;
  return (
    <span className="d-anchor" ref={anchor}>
      <TopbarEndButton element={element} open={open} onClick={() => setOpen((was) => !was)} />
      <AnchoredPanel
        anchor={anchor}
        open={open}
        onClose={close}
        align="end"
        matchWidth={false}
        role="dialog"
        aria-label={contribution.label}
        id={id}
        className="d-pop--panel sh-tb__panel"
      >
        <div className="sh-tb__panel-body" data-testid={`extension-topbar-end-panel-${element.key}`}>
          <ExtensionBoundary extensionId={extension.id}>
            {Component && (
              <Component extension={{ id: extension.id, name: extension.name }} context={context} close={close} />
            )}
          </ExtensionBoundary>
        </div>
      </AnchoredPanel>
    </span>
  );
}
