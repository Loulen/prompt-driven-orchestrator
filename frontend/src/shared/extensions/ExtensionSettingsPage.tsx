import { useMemo } from "react";
import ExtensionBoundary from "./ExtensionBoundary";
import { useLocationContext, type ContributionElement } from "./catalogue";
import type { PageContext, PageProps, PageShell } from "./contract";
import { navigatePage } from "./deepLink";

/**
 * A page of the `settings/page` location: the content of its rail entry in the
 * Settings, behind its error boundary. It saves the way its extension decides,
 * never with the instance form; the Settings header carries its breadcrumb and
 * its actions (`shell`).
 */
export default function ExtensionSettingsPage({
  page,
  path,
  shell,
}: {
  page: ContributionElement<PageProps>;
  /** What follows `#/ext/<extension>/settings/page/`; `""` from its rail entry. */
  path: string;
  shell: PageShell;
}) {
  const extra = useMemo(() => ({ path }), [path]);
  const context = useLocationContext<PageContext>("settings/page", extra);
  const { Component, extension } = page;
  return (
    <div
      className="min-h-0 min-w-0 flex-1 overflow-y-auto px-7 py-5"
      data-testid={`settings-page-${page.key}`}
    >
      <ExtensionBoundary extensionId={extension.id}>
        {Component && (
          <Component
            extension={{ id: extension.id, name: extension.name }}
            context={context}
            navigate={navigatePage}
            shell={shell}
          />
        )}
      </ExtensionBoundary>
    </div>
  );
}
