import { useEffect, useId, useState } from "react";
import { cssColor } from "../lib/cssColor";
import { useTheme } from "../hooks/useTheme";

// mermaid bakes colours into the generated SVG's own <style>, so a CSS `var()`
// would not resolve inside it — the values must be RESOLVED hex. #759: they are
// resolved from the live palette at initialize() time instead of being frozen
// dark, and `ensureMermaid` re-initializes when the theme changes, so a diagram
// re-rendered after a switch comes back in the new palette.
function themeVars() {
  const c = (token: `--color-${string}`, fallback: string) => cssColor(token, fallback);
  return {
    background: c("--color-bg-2", "#14171d"), // modal body
    primaryColor: c("--color-bg-3", "#1a1e25"), // node fill
    primaryBorderColor: c("--color-line-strong", "#2a313b"),
    primaryTextColor: c("--color-fg", "#e6e8eb"),
    secondaryColor: c("--color-bg-4", "#232831"),
    secondaryBorderColor: c("--color-line-strong", "#2a313b"),
    secondaryTextColor: c("--color-fg-2", "#aab1bd"),
    tertiaryColor: c("--color-bg-1", "#0f1115"),
    tertiaryBorderColor: c("--color-line", "#1f242c"),
    tertiaryTextColor: c("--color-fg-2", "#aab1bd"),
    lineColor: c("--color-fg-3", "#767e8c"), // edge stroke
    textColor: c("--color-fg-2", "#aab1bd"),
    mainBkg: c("--color-bg-3", "#1a1e25"),
    nodeBorder: c("--color-line-strong", "#2a313b"),
    nodeTextColor: c("--color-fg", "#e6e8eb"),
    edgeLabelBackground: c("--color-bg-2", "#14171d"), // opaque chip so labels stay legible
    noteBkgColor: c("--color-bg-4", "#232831"),
    noteBorderColor: c("--color-line-strong", "#2a313b"),
    noteTextColor: c("--color-fg", "#e6e8eb"),
    clusterBkg: c("--color-bg-1", "#0f1115"),
    clusterBorder: c("--color-line", "#1f242c"),
    activeTaskBkgColor: c("--color-acc", "#10b981"),
    titleColor: c("--color-fg", "#e6e8eb"),
    fontSize: "12px",
  };
}

// Initialize ONCE at module scope. initialize() is synchronous (returns void).
// `secure` is set explicitly so a per-diagram %%{init}%% / frontmatter directive
// cannot downgrade securityLevel at runtime (ADR-0013). Under `strict`, click
// handlers are disabled, so bindFunctions is never needed.
let initializedFor: string | null = null;
async function ensureMermaid(theme: string) {
  const mermaid = (await import("mermaid")).default; // lazy → code-split out of initial bundle
  if (initializedFor !== theme) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      suppressErrorRendering: true,
      theme: "base",
      themeVariables: themeVars(),
      fontFamily: '"Geist", ui-sans-serif, system-ui, -apple-system, sans-serif',
      secure: [
        "secure",
        "securityLevel",
        "startOnLoad",
        "maxTextSize",
        "suppressErrorRendering",
        "maxEdges",
      ],
    });
    initializedFor = theme;
  }
  return mermaid;
}

export default function MermaidDiagram({ source }: { source: string }) {
  // #759: the resolved theme is a render input — switching it re-runs the effect,
  // which re-initializes mermaid and re-renders the diagram in the new palette.
  const { resolved } = useTheme();
  const rawId = useId();
  // useId() emits delimiter chars unsafe in the `#${id}` selector mermaid builds
  // internally: ':' in React 19.0, guillemets '«»' in 19.1+ (we ship 19.2). Strip
  // everything outside [a-zA-Z0-9_-]; the broad regex is intentionally version-proof.
  const id = "mermaid-" + rawId.replace(/[^a-zA-Z0-9_-]/g, "");
  const [svg, setSvg] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false; // StrictMode double-invoke + iter-nav races
    (async () => {
      try {
        const mermaid = await ensureMermaid(resolved);
        // parse is ASYNC. With suppressErrors it resolves `false` on bad input or a
        // truthy { diagramType } object on success — gate on truthiness, NOT === true.
        const ok = await mermaid.parse(source, { suppressErrors: true });
        if (cancelled) return;
        if (!ok) {
          setFailed(true);
          return;
        }
        // render is ASYNC, resolves { svg }. Needs a live DOM (getBBox) → fails under
        // jsdom (that's why the meaningful test layer is Playwright, not vitest).
        const { svg } = await mermaid.render(id, source);
        if (cancelled) return;
        setSvg(svg);
      } catch {
        if (!cancelled) setFailed(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [source, id, resolved]);

  if (failed) {
    // Graceful degrade: show the raw source as a code block (ADR-0013, surface-don't-mask).
    return (
      <pre data-testid="mermaid-error">
        <code>{source}</code>
      </pre>
    );
  }
  if (svg == null) return <div className="mermaid-block" aria-busy="true" />;
  // svg is already DOMPurify-sanitized internally by mermaid under `strict`
  // (foreignObject-preserving config). Do NOT add an app-level default DOMPurify pass.
  return (
    <div
      className="mermaid-block"
      data-testid="mermaid-diagram"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
