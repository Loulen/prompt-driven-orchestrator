import { Fragment, useMemo, type ReactNode } from "react";
import { useAppearancePreview, useElementMarks, type ElementMarks } from "../../hooks/useElementMarks";
import { contributionElementId, SURFACE_OF_LOCATION } from "../../lib/appearance";
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
 * At a location of a surface with Éléments masquables (`run/info`, see
 * `SURFACE_OF_LOCATION`), a section the Apparence hides is left out; a sample
 * preview of Settings › Appearance draws none.
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
  const all = useContributions<SectionProps<C>>(location, context);
  const marksOf = useElementMarks();
  const preview = useAppearancePreview();
  const surface = SURFACE_OF_LOCATION[location];
  const sections = useMemo(
    () =>
      all.flatMap((section) => {
        if (!surface) return [{ ...section, marks: undefined as ElementMarks | undefined }];
        const marks = marksOf(contributionElementId(surface, section.key));
        return marks ? [{ ...section, marks }] : [];
      }),
    [all, surface, marksOf],
  );
  // Each section's registrations carry its rank: the host saves in screen order.
  const forms = useMemo(
    () => (form ? sections.map(({ extension }, rank) => rankedForm(form, rank, extension.id)) : []),
    [form, sections],
  );
  if (sections.length === 0 || (surface && preview?.sample)) return null;
  const entity = (context as { entity?: SectionProps["entity"] }).entity;
  return (
    <>
      {sections.map(({ key, contribution, extension, Component, marks }, rank) =>
        Component ? (
          <Fragment key={key}>
            {wrap({
              key,
              label: contribution.label,
              extension,
              marks,
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
