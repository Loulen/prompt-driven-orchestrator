import { useEffect, useId, useState } from "react";

// Dark-only theme variables (ADR-0013). mermaid bakes colours into the generated
// SVG's <style>, so CSS `var()` won't resolve inside it — we pass resolved hex
// values mapped to the PDO palette (see frontend/src/index.css `@theme`).
const DARK_THEME_VARS = {
  background: "#14171d", // bg-2 (modal body)
  primaryColor: "#1a1e25", // bg-3 node fill
  primaryBorderColor: "#2a313b", // line-strong
  primaryTextColor: "#e6e8eb", // fg
  secondaryColor: "#232831", // bg-4
  secondaryBorderColor: "#2a313b",
  secondaryTextColor: "#aab1bd", // fg-2
  tertiaryColor: "#0f1115", // bg-1
  tertiaryBorderColor: "#1f242c", // line
  tertiaryTextColor: "#aab1bd",
  lineColor: "#767e8c", // fg-3 (edge stroke, visible on dark)
  textColor: "#aab1bd", // fg-2
  mainBkg: "#1a1e25",
  nodeBorder: "#2a313b",
  nodeTextColor: "#e6e8eb",
  edgeLabelBackground: "#14171d", // opaque chip so labels stay legible
  noteBkgColor: "#232831",
  noteBorderColor: "#2a313b",
  noteTextColor: "#e6e8eb",
  clusterBkg: "#0f1115",
  clusterBorder: "#1f242c",
  activeTaskBkgColor: "#10b981", // acc
  titleColor: "#e6e8eb",
  fontSize: "12px",
} as const;

// Initialize ONCE at module scope. initialize() is synchronous (returns void).
// `secure` is set explicitly so a per-diagram %%{init}%% / frontmatter directive
// cannot downgrade securityLevel at runtime (ADR-0013). Under `strict`, click
// handlers are disabled, so bindFunctions is never needed.
let initialized = false;
async function ensureMermaid() {
  const mermaid = (await import("mermaid")).default; // lazy → code-split out of initial bundle
  if (!initialized) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      suppressErrorRendering: true,
      theme: "base",
      themeVariables: DARK_THEME_VARS,
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
    initialized = true;
  }
  return mermaid;
}

export default function MermaidDiagram({ source }: { source: string }) {
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
        const mermaid = await ensureMermaid();
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
  }, [source, id]);

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
