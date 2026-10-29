import Markdown from "react-markdown";
import { MARKDOWN_COMPONENTS, REMARK_PLUGINS } from "../../shared/markdown/plugins";
import type { SkillDetail } from "../model/skill";

/**
 * The SKILL.md tab of a skill's detail: the frontmatter as a read-only table,
 * in the author's key order, then the rendered body.
 */
export default function SkillMdTab({
  current,
  frontmatter,
  frontmatterKeys,
}: {
  /** The detail of the selected skill; `null` while it loads. */
  current: SkillDetail | null;
  frontmatter: Record<string, unknown> | null;
  frontmatterKeys: string[];
}) {
  return (
    <div className="mt-3 flex flex-col gap-4">
      {frontmatter && frontmatterKeys.length > 0 && (
        <div className="overflow-hidden rounded-md border border-line" data-testid="skill-frontmatter">
          <div className="flex items-center justify-between bg-surface-2 px-3 py-1.5 text-text-3 uppercase tracking-wide text-2xs">
            <span>Frontmatter</span>
            <span>Read-only</span>
          </div>
          <table className="w-full text-xs">
            <tbody>
              {frontmatterKeys.map((key) => {
                const value = frontmatter[key];
                return (
                <tr key={key} className="border-t border-line">
                  <td className="w-[160px] px-3 py-1.5 align-top font-mono text-text-3">{key}</td>
                  <td className="px-3 py-1.5 font-mono text-text">
                    {typeof value === "string" ? value : JSON.stringify(value)}
                  </td>
                </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {current === null ? (
        <div className="text-text-3 text-xs">
          Loading…
        </div>
      ) : current.body === null ? (
        <div className="rounded-md border border-st-blocked/40 bg-st-blocked/14 px-3 py-2 text-text-2 text-xs">
          The <span className="font-mono">SKILL.md</span> of this skill is missing on disk (
          <span className="font-mono">{current.path}</span>). Delete the entry and paste it again.
        </div>
      ) : (
        <div className="artifact-markdown prose-sm text-text-2 [&_ul]:list-disc [&_ol]:list-decimal [&_ul]:pl-5 [&_ol]:pl-5 text-sm" data-testid="skill-body">
          <Markdown remarkPlugins={REMARK_PLUGINS} components={MARKDOWN_COMPONENTS}>{current.body}</Markdown>
        </div>
      )}
    </div>
  );
}
