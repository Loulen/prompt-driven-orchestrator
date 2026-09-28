import type { ReactNode } from "react";
import type { SettingsSection } from "../../model/settingsCategories";

/** One section of a category page: its heading, its badges, its description, its body. */
export default function Section({ section, children }: { section: SettingsSection; children: ReactNode }) {
  return (
    <section
      id={`settings-section-${section.id}`}
      data-section-id={section.id}
      data-testid={`settings-section-body-${section.id}`}
      className="settings-section flex flex-col gap-4"
      style={{ scrollMarginTop: 16 }}
    >
      <div className="flex flex-col gap-1">
        <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13px" }}>
          {section.label}
          {section.readOnly && (
            <span
              className="rounded bg-bg-3 px-1.5 py-0.5 font-normal text-fg-4"
              style={{ fontSize: "9.5px" }}
            >
              read-only
            </span>
          )}
          {section.ownPersistence && (
            <span
              className="rounded bg-bg-3 px-1.5 py-0.5 font-normal text-fg-4"
              style={{ fontSize: "9.5px" }}
              title="Each edit is written when you make it. The Save button below does not apply here."
            >
              saves as you go
            </span>
          )}
          {section.deviceLocal && (
            <span
              className="rounded-full border border-acc-border bg-acc-bg px-2 py-0.5 font-normal text-acc"
              style={{ fontSize: "9.5px" }}
              data-testid={`settings-section-${section.id}-device-local`}
            >
              Device-local · saved immediately
            </span>
          )}
        </h3>
        <p className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {section.description}
        </p>
      </div>
      {children}
    </section>
  );
}
