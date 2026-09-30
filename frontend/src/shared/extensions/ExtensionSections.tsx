import { Fragment, useMemo, type ReactNode } from "react";
import type { UiLocation } from "../generated/UiLocation";
import { useContributions, useLocationContext } from "./catalogue";
import type { LocationContext, SectionForm, SectionProps } from "./contract";
import ExtensionBoundary from "./ExtensionBoundary";
import { rankedForm } from "./sectionForm";
import { modalSection, type RenderedSection } from "./sectionLayout";

export type { RenderedSection } from "./sectionLayout";

/**
 * A section location (`project/edit`, `run/new`, `node/config`,
 * `pipeline/config`, `run/info`): one section per contribution, extensions in
 * registration order, each behind its own error boundary. Nothing without a
 * contribution. `wrap` draws a section the way its host draws its own.
 *
 * `extra` is what the location adds to the base context; memoise it. `form` is
 * the host's `useSectionForm().form`, where the host has a Save.
 */
export default function ExtensionSections<C extends LocationContext>({
  location,
  extra,
  form,
  wrap = modalSection,
}: {
  location: UiLocation;
  extra: Omit<C, keyof LocationContext>;
  form?: SectionForm;
  wrap?: (section: RenderedSection) => ReactNode;
}) {
  const context = useLocationContext<C>(location, extra);
  const sections = useContributions<SectionProps<C>>(location, context);
  // Each section's registrations carry its rank: the host saves in screen order.
  const forms = useMemo(
    () => (form ? sections.map((_, rank) => rankedForm(form, rank)) : []),
    [form, sections],
  );
  if (sections.length === 0) return null;
  const entity = (context as { entity?: SectionProps["entity"] }).entity;
  return (
    <>
      {sections.map(({ key, contribution, extension, Component }, rank) =>
        Component ? (
          <Fragment key={key}>
            {wrap({
              key,
              label: contribution.label,
              extension,
              body: (
                <ExtensionBoundary extensionId={extension.id}>
                  <Component
                    extension={{ id: extension.id, name: extension.name }}
                    context={context}
                    entity={entity}
                    form={forms[rank]}
                  />
                </ExtensionBoundary>
              ),
            })}
          </Fragment>
        ) : null,
      )}
    </>
  );
}
