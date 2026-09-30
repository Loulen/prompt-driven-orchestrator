import type { ReactNode } from "react";
import type { ExtensionEntry } from "../generated/ExtensionEntry";

export interface RenderedSection {
  key: string;
  label: string;
  extension: ExtensionEntry;
  /** The section's component, behind its error boundary. */
  body: ReactNode;
}

/** A modal's section (`project/edit`, `run/new`): a rule above, the label and the extension's name. */
export function modalSection({ key, label, extension, body }: RenderedSection) {
  return (
    <section className="mb-3 border-t border-line pt-3" data-testid={`extension-section-${key}`}>
      <div className="mb-2 flex items-center gap-1.5 font-medium text-fg" style={{ fontSize: "12px" }}>
        {label}
        <span className="font-normal text-fg-4" style={{ fontSize: "10.5px" }}>
          · {extension.name}
        </span>
      </div>
      {body}
    </section>
  );
}

