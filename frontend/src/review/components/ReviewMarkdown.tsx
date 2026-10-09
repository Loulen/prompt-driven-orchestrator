import Markdown from "react-markdown";
import { useCompletionMarkdown } from "../../shared/extensions/completionRender";

/**
 * The markdown of a review comment, a reply or the editor's preview: the
 * core's render, plus what the sources of completion of `review/composer`
 * draw of the texts they inserted (CORE-63).
 */
export default function ReviewMarkdown({ text }: { text: string }) {
  const { remarkPlugins, components } = useCompletionMarkdown("review/composer");
  return (
    <Markdown remarkPlugins={remarkPlugins} components={components}>
      {text}
    </Markdown>
  );
}
