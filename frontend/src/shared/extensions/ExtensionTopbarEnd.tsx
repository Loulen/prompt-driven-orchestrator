import { Tooltip } from "../ui/kit";
import { useContributions, useLocationContext, type ContributionElement } from "./catalogue";
import { runCommand } from "./commands";
import { openPage } from "./deepLink";
import ExtensionBoundary from "./ExtensionBoundary";
import ExtensionIcon from "./ExtensionIcon";
import { usePageCounter } from "./pageCounter";

/**
 * The `topbar/end` location: one icon button per element, in the top bar's
 * right-hand group, before the core's icons, a hairline after them. A page's
 * button opens it full-window; an action's calls its command with the
 * location's context. A page that declares a counter (0.19) shows it on its
 * icon, for the current Actor, live. Nothing without a contribution.
 */
export default function ExtensionTopbarEnd() {
  const context = useLocationContext("topbar/end");
  const elements = useContributions("topbar/end", context);
  if (elements.length === 0) return null;
  return (
    <>
    {/* The icons shrink out of sight first, the hairline after them stays. */}
    <div className="sh-tb__ext" role="group" aria-label="Extensions" data-testid="extension-topbar-end">
      {elements.map((element) => (
        <ExtensionBoundary key={element.key} extensionId={element.extension.id}>
          <TopbarEndButton element={element} onClick={() => activate(element, context)} />
        </ExtensionBoundary>
      ))}
    </div>
    <span className="sh-tb__sep" aria-hidden />
    </>
  );
}

function activate(element: ContributionElement, context: ReturnType<typeof useLocationContext>) {
  const { contribution, extension } = element;
  if (contribution.type === "page") {
    openPage({ key: element.key, extension: extension.id, location: "topbar/end", path: "" });
  } else {
    void runCommand(element, context);
  }
}

function TopbarEndButton({ element, onClick }: { element: ContributionElement; onClick: () => void }) {
  const { contribution, command, extension } = element;
  const icon = contribution.type === "page" ? contribution.icon : command?.icon;
  const label = contribution.label;
  const counter = usePageCounter(extension.id, contribution.type === "page" ? contribution.counter : null);
  const count = counter && counter.count > 0 ? counter.count : null;
  return (
    <Tooltip label={label} description={count ? (counter?.detail ?? undefined) : undefined}>
      <button
        type="button"
        onClick={onClick}
        aria-label={count ? `${label}, ${count}` : label}
        data-testid={`extension-topbar-end-${element.key}`}
        className="d-icon"
      >
        <ExtensionIcon url={icon} label={label} />
        {count && (
          <span
            className="sh-tb__count"
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
