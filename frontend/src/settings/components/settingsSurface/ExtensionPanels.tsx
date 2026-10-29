import { useMemo } from "react";
import { createPortal } from "react-dom";
import ExtensionBoundary from "../../../shared/extensions/ExtensionBoundary";
import ExtensionIcon from "../../../shared/extensions/ExtensionIcon";
import { useLocationContext, useSettingsPages, useSettingsPanels } from "../../../shared/extensions/catalogue";
import { PageContext } from "./search";
import Section from "./Section";

/**
 * The `settings` location (the 0.1 settings-panel slot): the Extensions page of the
 * Settings surface, one section per contribution, headed like the core's sections with the
 * extension that added it and the « saves as you go » mark — each saves the way its
 * extension decides, never with the instance form. Its sections are listed in the
 * surface's section column (`navSlot`) while it is on screen.
 */
export default function ExtensionPanels({
  active,
  navSlot,
}: {
  active: boolean;
  navSlot?: HTMLElement | null;
}) {
  const panels = useSettingsPanels();
  const pages = useSettingsPages();
  const context = useLocationContext("settings");
  const page = useMemo(() => ({ dirty: new Set<string>(), category: "Extensions" }), []);
  // A section contribution has no icon of its own: its extension's page lends one.
  const iconOf = (extensionId: string) => {
    const owned = pages.find((item) => item.extension.id === extensionId)?.contribution;
    return owned?.type === "page" ? (owned.icon ?? null) : null;
  };
  return (
    <>
      {active &&
        navSlot &&
        createPortal(
          <nav aria-label="Extensions sections" data-testid="settings-subcolumn">
            {panels.map(({ key, contribution }) => (
              <button
                key={key}
                type="button"
                className="set-nav__item"
                title={contribution.label}
                onClick={() =>
                  document
                    .getElementById(`settings-section-${key}`)
                    ?.scrollIntoView?.({ behavior: "smooth", block: "start" })
                }
              >
                <span className="set-nav__label">{contribution.label}</span>
              </button>
            ))}
          </nav>,
          navSlot,
        )}
      <div hidden={!active} className={active ? "set-page" : undefined} data-testid="settings-page-extensions">
        <div className="set-page__col">
          <PageContext.Provider value={page}>
            {panels.map(({ key, contribution, extension, Component }) => (
              <div key={key} data-testid={`extension-settings-${key}`}>
                <Section
                  section={{ id: key, label: contribution.label, ownPersistence: true }}
                  badge={
                    <span
                      className="set-sec__by"
                      title={`Added by the extension ${extension.name} ${extension.version}`}
                    >
                      <ExtensionIcon url={iconOf(extension.id)} label={extension.name} size={12} />
                      <span className="set-ellipsis">{extension.name}</span>
                    </span>
                  }
                >
                  <div className="set-extra">
                    <ExtensionBoundary extensionId={extension.id}>
                      {Component && (
                        <Component extension={{ id: extension.id, name: extension.name }} context={context} />
                      )}
                    </ExtensionBoundary>
                  </div>
                </Section>
              </div>
            ))}
          </PageContext.Provider>
        </div>
      </div>
    </>
  );
}
