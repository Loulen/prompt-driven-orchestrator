import ExtensionBoundary from "./ExtensionBoundary";
import { useLocationContext, useSettingsPanels } from "./catalogue";

/**
 * The `settings` location (the 0.1 settings-panel slot): the Extensions page of
 * the Settings surface, one section per contribution, headed like the other
 * sections. Each section saves the way its extension decides, never with the
 * instance form.
 */
export default function ExtensionSettingsPanels({ active }: { active: boolean }) {
  const panels = useSettingsPanels();
  const context = useLocationContext("settings");
  return (
    <div
      hidden={!active}
      className={active ? "flex min-h-0 min-w-0 flex-1" : undefined}
      data-testid="settings-page-extensions"
    >
      <div className="min-w-0 flex-1 overflow-y-auto px-7 py-5">
        <div className="flex max-w-[640px] flex-col gap-7 pb-10">
          {panels.map(({ key, contribution, extension, Component }) => (
            <section
              key={key}
              className="settings-section flex flex-col gap-4"
              data-testid={`extension-settings-${key}`}
            >
              <div className="flex flex-col gap-1">
                <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13px" }}>
                  {contribution.label}
                </h3>
                <p className="text-fg-4" style={{ fontSize: "10.5px" }}>
                  Added by the extension {extension.name} {extension.version}.
                </p>
              </div>
              <ExtensionBoundary extensionId={extension.id}>
                {Component && (
                  <Component extension={{ id: extension.id, name: extension.name }} context={context} />
                )}
              </ExtensionBoundary>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
