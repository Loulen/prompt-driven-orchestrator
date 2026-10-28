import { Tooltip } from "../ui/tooltip";
import { useContributions, useLocationContext, type ContributionElement } from "./catalogue";
import { runCommand } from "./commands";
import { openPage } from "./deepLink";
import ExtensionBoundary from "./ExtensionBoundary";
import ExtensionIcon from "./ExtensionIcon";

/**
 * The `topbar/end` location: one icon button per element, in the top bar's
 * right-hand group, before Stats. A page's button opens it full-window; an
 * action's calls its command with the location's context. Nothing without a
 * contribution.
 */
export default function ExtensionTopbarEnd() {
  const context = useLocationContext("topbar/end");
  const elements = useContributions("topbar/end", context);
  if (elements.length === 0) return null;
  return (
    <div className="flex items-center gap-1" data-testid="extension-topbar-end">
      {elements.map((element) => (
        <ExtensionBoundary key={element.key} extensionId={element.extension.id}>
          <TopbarEndButton element={element} onClick={() => activate(element, context)} />
        </ExtensionBoundary>
      ))}
    </div>
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
  const { contribution, command } = element;
  const icon = contribution.type === "page" ? contribution.icon : command?.icon;
  const label = contribution.label;
  return (
    <Tooltip content={label}>
      <button
        type="button"
        onClick={onClick}
        aria-label={label}
        data-testid={`extension-topbar-end-${element.key}`}
        className="grid h-6 w-6 place-items-center rounded text-text-3 transition-colors hover:bg-active hover:text-text"
      >
        <ExtensionIcon url={icon} label={label} />
      </button>
    </Tooltip>
  );
}
