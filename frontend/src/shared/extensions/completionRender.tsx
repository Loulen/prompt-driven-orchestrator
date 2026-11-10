/**
 * The render of what a source of completion inserted (API 0.14): in every
 * markdown render of a location's texts, each match of a source's `pattern`
 * is drawn by the component its bundle exports, behind its error boundary
 * (a failing one shows the plain text). Text in a link or in code is left
 * alone. Without a source that renders, the core's markdown is unchanged.
 */

import { useMemo, type ReactNode } from "react";
import type { Components } from "react-markdown";
import { MARKDOWN_COMPONENTS, REMARK_PLUGINS } from "../markdown/plugins";
import type { UiLocation } from "../generated/UiLocation";
import { useContributions, useLocationContext, type ContributionElement } from "./catalogue";
import { compilePattern, splitRendered } from "./completion";
import ExtensionBoundary from "./ExtensionBoundary";

/** The element a match becomes in the markdown tree. */
const MATCH_TAG = "pdo-completion-match";

interface MdNode {
  type: string;
  value?: string;
  children?: MdNode[];
  data?: { hName: string; hProperties: Record<string, string> };
}

/** Nodes whose text is never drawn by a source. */
const OPAQUE = new Set(["link", "linkReference", "inlineCode", "code", "html", "definition"]);

type Render = { source: string; pattern: RegExp };

function cut(node: MdNode, renders: Render[]): void {
  if (!node.children || OPAQUE.has(node.type)) return;
  node.children = node.children.flatMap((child): MdNode[] => {
    if (child.type !== "text" || !child.value) {
      cut(child, renders);
      return [child];
    }
    return splitRendered(child.value, renders).map((piece) =>
      piece.kind === "text"
        ? { type: "text", value: piece.text }
        : {
            type: "completionMatch",
            data: { hName: MATCH_TAG, hProperties: { dataSource: piece.source, dataText: piece.text } },
            children: [{ type: "text", value: piece.text }],
          },
    );
  });
}

/** A remark plugin turning the matches of `renders` into elements the render draws. */
export function remarkCompletionMatches(renders: Render[]) {
  return () => (tree: MdNode) => cut(tree, renders);
}

interface Markdown {
  remarkPlugins: typeof REMARK_PLUGINS;
  components: Components;
}

const PLAIN: Markdown = { remarkPlugins: REMARK_PLUGINS, components: MARKDOWN_COMPONENTS };

/**
 * The plugins and components of a markdown render of `location`'s texts: the
 * core's, plus the renders of its sources of completion.
 */
export function useCompletionMarkdown(location: UiLocation): Markdown {
  const context = useLocationContext(location);
  const all = useContributions(location, context);
  return useMemo(() => {
    const sources = all.filter((e) => e.completion?.render);
    const renders = sources.flatMap((element): Render[] => {
      const pattern = compilePattern(element.completion!.render!.pattern);
      if (!pattern) {
        console.warn(`extension ${element.extension.id}: the render pattern of ${element.contribution.id} is invalid`);
        return [];
      }
      return [{ source: element.key, pattern }];
    });
    if (renders.length === 0) return PLAIN;
    const byKey = new Map<string, ContributionElement>(sources.map((e) => [e.key, e]));
    const Match = (props: { children?: ReactNode; "data-source"?: string; "data-text"?: string }) => {
      const element = byKey.get(props["data-source"] ?? "");
      const Render = element?.completion?.render?.Component;
      const text = props["data-text"] ?? "";
      if (!element || !Render) return <>{props.children}</>;
      return (
        <span data-testid={`completion-match-${element.key}`}>
          <ExtensionBoundary extensionId={element.extension.id} fallback={text}>
            <Render extension={{ id: element.extension.id, name: element.extension.name }} context={context} text={text} />
          </ExtensionBoundary>
        </span>
      );
    };
    return {
      remarkPlugins: [...REMARK_PLUGINS, remarkCompletionMatches(renders)] as typeof REMARK_PLUGINS,
      components: { ...MARKDOWN_COMPONENTS, [MATCH_TAG]: Match } as Components,
    };
  }, [all, context]);
}
