import type { ReactNode } from "react";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { ElementMarks } from "../../hooks/useElementMarks";
import ExtensionIcon from "./ExtensionIcon";

export interface RenderedSection {
  key: string;
  label: string;
  extension: ExtensionEntry;
  /** The section's component, behind its error boundary. */
  body: ReactNode;
  /** At a location of a surface with Éléments masquables: the preview's marks of the section. */
  marks?: ElementMarks;
}

/** The icon an extension shows for itself: the one of its first page, if any. */
export function extensionIconUrl(extension: ExtensionEntry): string | null {
  for (const c of extension.ui?.contributions ?? []) {
    if (c.type === "page" && c.icon) return c.icon;
  }
  return null;
}

/**
 * A modal's section (`project/edit`, `run/new`), drawn like the host's own:
 * a rule above, the label as a caps tag with the extension's icon (its name in
 * the tooltip), the body in the control column.
 */
export function modalSection({ key, label, extension, body }: RenderedSection) {
  return (
    <section className="ln-sec" data-testid={`extension-section-${key}`}>
      <span className="d-caps ln-sec__tag" title={`${label} · ${extension.name}`}>
        {label}
        <span className="ln-sec__ext" aria-label={extension.name} role="img">
          <ExtensionIcon url={extensionIconUrl(extension)} label={extension.name} size={11} />
        </span>
      </span>
      <div className="ln-row">
        <span className="ln-row__label" />
        <div className="ln-row__ctl ln-row__ctl--block">{body}</div>
      </div>
    </section>
  );
}
