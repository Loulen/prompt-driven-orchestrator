import { useEffect, useId, useState } from "react";
import { cssColor, cssToken } from "../../lib/cssColor";
import { useTheme } from "../../hooks/useTheme";

// mermaid bakes colours into the generated SVG's own <style>, so a CSS `var()`
// would not resolve inside it — the values must be RESOLVED colours. #759: they are
// resolved from the live palette at initialize() time instead of being frozen
// dark, and `ensureMermaid` re-initializes when the theme changes, so a diagram
// re-rendered after a switch comes back in the new palette.
function themeVars() {
  const c = (token: `--${string}`) => cssColor(token);
  const vars: Record<string, string> = {
    background: c("--surface-1"), // modal body
    primaryColor: c("--surface-2"), // node fill
    primaryBorderColor: c("--line-strong"),
    primaryTextColor: c("--text"),
    secondaryColor: c("--surface-2"),
    secondaryBorderColor: c("--line-strong"),
    secondaryTextColor: c("--text-2"),
    tertiaryColor: c("--surface-0"),
    tertiaryBorderColor: c("--line"),
    tertiaryTextColor: c("--text-2"),
    lineColor: c("--text-3"), // edge stroke
    textColor: c("--text-2"),
    mainBkg: c("--surface-2"),
    nodeBorder: c("--line-strong"),
    nodeTextColor: c("--text"),
    edgeLabelBackground: c("--surface-1"), // opaque chip so labels stay legible
    noteBkgColor: c("--surface-2"),
    noteBorderColor: c("--line-strong"),
    noteTextColor: c("--text"),
    clusterBkg: c("--surface-0"),
    clusterBorder: c("--line"),
    activeTaskBkgColor: c("--primary"),
    titleColor: c("--text"),
  };
  // The labels' size: the type scale's secondary text, read live like the colours.
  vars.fontSize = cssToken("--fs-sm", "12px");
  return vars;
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
      fontFamily: '"Archivo", ui-sans-serif, system-ui, sans-serif',
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
