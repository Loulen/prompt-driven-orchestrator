import { useMemo } from "react";
import type { Components } from "react-markdown";
import type { Element } from "hast";
import MermaidDiagram from "../../../shared/ui/MermaidDiagram";
import MarkdownLink from "../../../shared/markdown/MarkdownLink";

/**
 * The markdown overrides of the artifact viewer: external links open a new tab,
 * deep links open their extension's page, an embedded image zooms, a ```mermaid
 * fence renders as a diagram.
 */
export function useMarkdownComponents(
  onZoom: (images: string[], index: number) => void,
): Components {
  // #369: memoise the markdown component overrides so their identities survive a
  // poll-driven re-render (see the module-scope note on REMARK_PLUGINS). `onZoom`
  // must be stable (the modal hands a stable useState setter), which keeps the
  // `pre`/`img` references constant for the modal's whole lifetime — no remount of
  // the routed <MermaidDiagram>, hence no flicker.
  return useMemo<Components>(
    () => ({
      // #698: an absolute http(s) link leaves the app in a new tab; #29: a deep link
      // (`#/ext/…`) opens its extension's page. Relative links stay.
      a: MarkdownLink,
      img: ({ src, alt }) => {
        const url = typeof src === "string" ? src : undefined;
        return (
          <img
            src={url}
            alt={alt ?? ""}
            className="cursor-zoom-in rounded transition-opacity hover:opacity-90"
            onClick={() => {
              // A markdown-embedded <img> is rendered in isolation by
              // react-markdown — there is no collected list to page through, so it
              // is an honest single-image set.
              if (url) onZoom([url], 0);
            }}
          />
        );
      },
      // A ```mermaid fenced block parses to
      // <pre><code class="language-mermaid">src</code></pre>. Detect it on the
      // child <code> and unwrap to a rendered SVG diagram; every other block falls
      // through to a plain <pre>. We override `pre` (not `code`) to avoid emitting a
      // <div> inside a <pre> (invalid nesting). Regular ```ts / ```bash fences are
      // untouched. (#240)
      pre: ({ node, children, ...rest }) => {
        const child = node?.children?.[0];
        const isMermaid =
          child?.type === "element" &&
          child.tagName === "code" &&
          ((child.properties?.className as string[]) ?? []).includes(
            "language-mermaid",
          );
        if (isMermaid) {
          const codeEl = child as Element;
          const text = codeEl.children?.[0];
          const source =
            text && text.type === "text" ? text.value.replace(/\n$/, "") : "";
          return <MermaidDiagram source={source} />;
        }
        return <pre {...rest}>{children}</pre>;
      },
    }),
    [onZoom],
  );
}
