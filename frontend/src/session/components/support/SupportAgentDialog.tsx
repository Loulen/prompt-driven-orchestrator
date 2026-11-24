import { useMemo, useState } from "react";
import { CircleHelp, TriangleAlert } from "lucide-react";
import type { AgentChoice } from "../../../shared/generated/AgentChoice";
import { Banner, Button, TextArea } from "../../../shared/ui/kit";
import { Modal } from "../../../shared/ui/kit/Modal";
import AgentControl from "../../../settings/components/AgentControl";
import { useAgentProfiles } from "../../../settings/store/useAgentProfiles";
import { useInstanceAgent } from "../../store/useInstanceAgent";
import { useSupportAgent } from "../../store/supportAgentStore";
import {
  choiceCombination,
  choiceLabel,
  initialChoice,
  installedCatalog,
  runnableProfiles,
} from "../../model/supportAgent";

/**
 * « Open support agent »: the agent (a profile, or a custom combination, on a
 * harness installed here; the last choice preselected) and an optional
 * question. Enter opens with the choice shown. With an agent open, the window
 * says that opening another ends its conversation, and offers to ask the open
 * one instead.
 */
export default function SupportAgentDialog({ question: initialQuestion }: { question: string }) {
  const status = useSupportAgent((s) => s.status);
  const pending = useSupportAgent((s) => s.pending);
  const hide = useSupportAgent((s) => s.hideDialog);
  const open = useSupportAgent((s) => s.open);
  const ask = useSupportAgent((s) => s.ask);
  const { profiles: allProfiles, loaded: profilesLoaded } = useAgentProfiles(true);
  const { catalog: fullCatalog, loaded: catalogLoaded } = useInstanceAgent();
  // Until both answer, the last choice cannot be told runnable: nothing opens.
  const loaded = profilesLoaded && catalogLoaded;
  const catalog = useMemo(() => installedCatalog(fullCatalog), [fullCatalog]);
  const profiles = useMemo(() => runnableProfiles(allProfiles, fullCatalog), [allProfiles, fullCatalog]);

  const [picked, setPicked] = useState<AgentChoice | null>(null);
  const choice = picked ?? (loaded ? initialChoice(status?.last_choice, allProfiles, fullCatalog) : null);
  const [question, setQuestion] = useState(initialQuestion);
  const asking = question.trim().length > 0;
  const openAgent = status?.alive ? status.agent : null;
  const openName = openAgent?.harness ?? "the agent";

  const submit = () => {
    // The store's flag, not this render's: a second Enter before the re-render opens nothing.
    if (!choice || useSupportAgent.getState().pending) return;
    void open(choice, question, !!status?.alive);
  };
  const askOpen = () => {
    if (asking) void ask(question.trim());
    else {
      hide();
      useSupportAgent.getState().expand();
    }
  };

  return (
    <Modal
      size="s"
      icon={<CircleHelp size={14} strokeWidth={1.5} aria-hidden />}
      title="Open support agent"
      onClose={hide}
      onSubmit={submit}
      data-testid="support-agent-dialog"
      footerNote={status?.alive ? undefined : "Enter: open with the last choice"}
      actions={
        <>
          <Button onClick={hide}>Cancel</Button>
          {status?.alive && (
            <Button onClick={askOpen} disabled={pending} data-testid="support-agent-ask-open">
              {asking ? `Ask ${openName} instead` : `Back to ${openName}`}
            </Button>
          )}
          <Button tone="primary" onClick={submit} disabled={!choice || pending} data-testid="support-agent-submit">
            {status?.alive ? `Close ${openName} and open` : asking ? "Ask" : "Open"}
          </Button>
        </>
      }
    >
      <div className="sa-form">
        {status?.alive && (
          <Banner
            tone="awaiting"
            icon={TriangleAlert}
            title={`${openName} is open`}
            text={`Opening ${choiceLabel(choice, allProfiles)} ends its conversation.`}
          />
        )}
        <div className="sa-form__field">
          <span className="sa-form__label">Agent</span>
          {!loaded ? (
            <span className="sa-none">Loading the agents…</span>
          ) : choice ? (
            <AgentControl
              choice={choice}
              onChange={setPicked}
              profiles={profiles}
              catalog={catalog}
              inherited={choiceCombination(choice, allProfiles)}
              allowInherit={false}
              showLabel={false}
              label="Agent"
              testId="support-agent-choice"
            />
          ) : (
            <span className="sa-none">No harness is installed on this machine.</span>
          )}
        </div>
        <div className="sa-form__field">
          <label className="sa-form__label" htmlFor="support-agent-question">
            Question
          </label>
          <TextArea
            id="support-agent-question"
            data-autofocus
            rows={4}
            value={question}
            placeholder="Optional: ask it now, or open it and ask later"
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => {
              // Enter opens; Shift+Enter starts a new line.
              // Ctrl+Enter is the window's own (it submits once, from the Modal).
              if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                submit();
              }
            }}
            data-testid="support-agent-question"
          />
        </div>
      </div>
    </Modal>
  );
}
