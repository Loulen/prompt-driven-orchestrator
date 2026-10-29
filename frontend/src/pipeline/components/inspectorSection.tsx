import { Puzzle } from "lucide-react";
import type { RenderedSection } from "../../shared/extensions/ExtensionSections";
import { Section } from "../../shared/ui/kit";
import "./inspect.css";

/**
 * One contributed section, drawn as the host's own: the kit's caps-head
 * Section titled with its label, the extension's name at the right of the head.
 */
export function inspectorSection({ key, label, extension, body }: RenderedSection) {
  return (
    <div key={key} data-testid={`extension-section-${key}`}>
      <Section
        title={label}
        aside={
          <span className="ei-keep ei-dim" title={`Extension ${extension.name}`}>
            <Puzzle size={11} strokeWidth={1.5} aria-hidden />
            {extension.name}
          </span>
        }
      >
        {body}
      </Section>
    </div>
  );
}
