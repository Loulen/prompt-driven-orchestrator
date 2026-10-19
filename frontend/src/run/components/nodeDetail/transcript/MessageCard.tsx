import { useState } from "react";
import Markdown from "react-markdown";
import { Bot, User } from "lucide-react";
import type { TranscriptSpeaker } from "../../../../shared/generated/TranscriptSpeaker";
import { MARKDOWN_COMPONENTS, REMARK_PLUGINS } from "../../../../shared/markdown/plugins";
import { formatTime } from "../../../model/nodeDetailFormat";

/** Past this many characters a message is folded behind « Show more ». */
export const MESSAGE_FOLD = 1200;

/** A visible message of the transcript: a Prompt (the user's turn) or the
 *  Agent's reply, in a card. */
export default function MessageCard({
  speaker,
  text,
  at,
}: {
  speaker: TranscriptSpeaker;
  text: string;
  at?: string;
}) {
  const [open, setOpen] = useState(false);
  const long = text.length > MESSAGE_FOLD;
  const shown = long && !open ? `${text.slice(0, MESSAGE_FOLD)}…` : text;
  const Icon = speaker === "prompt" ? User : Bot;
  return (
    <div
      data-testid={`transcript-message-${speaker}`}
      className={`rounded-md border px-2.5 py-2 ${
        speaker === "prompt" ? "border-line-strong bg-bg-3" : "border-line bg-bg-2"
      }`}
    >
      <div className="mb-1 flex items-center gap-1.5 text-fg-3" style={{ fontSize: "10.5px" }}>
        <Icon size={11} className="text-fg-4" />
        {speaker === "prompt" ? "Prompt" : "Agent"}
        {at && (
          <span className="ml-auto font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {formatTime(at)}
          </span>
        )}
      </div>
      <div className="artifact-markdown break-words text-fg-2" style={{ fontSize: "11.5px", lineHeight: 1.5 }}>
        <Markdown remarkPlugins={REMARK_PLUGINS} components={MARKDOWN_COMPONENTS}>
          {shown}
        </Markdown>
      </div>
      {long && (
        <button
          type="button"
          data-testid="transcript-message-more"
          onClick={() => setOpen((o) => !o)}
          className="mt-1 cursor-pointer text-fg-4 hover:text-fg-2"
          style={{ fontSize: "10.5px" }}
        >
          {open ? "Show less" : "Show more"}
        </button>
      )}
    </div>
  );
}
