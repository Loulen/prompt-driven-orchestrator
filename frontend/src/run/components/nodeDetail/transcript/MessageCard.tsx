import { useState } from "react";
import Markdown from "react-markdown";
import { Bot, User } from "lucide-react";
import type { TranscriptSpeaker } from "../../../../shared/generated/TranscriptSpeaker";
import { MARKDOWN_COMPONENTS, REMARK_PLUGINS } from "../../../../shared/markdown/plugins";
import { rehypeMarkQuery } from "../../../../shared/markdown/rehypeMarkQuery";
import { formatTime } from "../../../model/nodeDetailFormat";
import { useFocusUnfold } from "../../../store/runSearchStore";

/** Past this many characters a message is folded behind « Show more ». */
export const MESSAGE_FOLD = 1200;

/** A visible message of the transcript: a Prompt (the user's turn) or the
 *  Agent's reply, in a card. The Run's search marks its word in it
 *  (`highlight`); the hit chosen there unfolds and outlines it (`focusNonce`,
 *  new at each choice). */
export default function MessageCard({
  speaker,
  text,
  at,
  index,
  highlight = "",
  focusNonce,
}: {
  speaker: TranscriptSpeaker;
  text: string;
  at?: string;
  index?: number;
  highlight?: string;
  focusNonce?: number;
}) {
  const [open, setOpen] = useState(false);
  useFocusUnfold(focusNonce, () => setOpen(true));
  const focused = focusNonce !== undefined;
  const long = text.length > MESSAGE_FOLD;
  const shown = long && !open ? `${text.slice(0, MESSAGE_FOLD)}…` : text;
  const Icon = speaker === "prompt" ? User : Bot;
  return (
    <article
      data-testid={`transcript-message-${speaker}`}
      data-entry={index}
      data-focused={focused || undefined}
      data-speaker={speaker}
      className="ri-tx__msg"
      style={focused ? { borderColor: "var(--color-st-await)" } : undefined}
    >
      <header className="ri-tx__mhead">
        <Icon size={12} strokeWidth={1.5} aria-hidden />
        <span className="ri-tx__who">{speaker === "prompt" ? "Prompt" : "Agent"}</span>
        {at && <time className="ri-tx__time">{formatTime(at)}</time>}
      </header>
      <div className="artifact-markdown ri-tx__md">
        <Markdown
          remarkPlugins={REMARK_PLUGINS}
          rehypePlugins={highlight ? [[rehypeMarkQuery, { query: highlight }]] : undefined}
          components={MARKDOWN_COMPONENTS}
        >
          {shown}
        </Markdown>
      </div>
      {long && (
        <button type="button" data-testid="transcript-message-more" onClick={() => setOpen((o) => !o)} className="ri-tx__more">
          {open ? "Show less" : "Show more"}
        </button>
      )}
    </article>
  );
}
