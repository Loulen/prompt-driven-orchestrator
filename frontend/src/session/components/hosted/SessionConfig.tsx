import { useState } from "react";
import { SquareTerminal } from "lucide-react";
import { Button, EmptyState, TextArea, toast } from "../../../shared/ui/kit";
import AgentControl from "../../../settings/components/AgentControl";
import { useAgentProfiles } from "../../../settings/store/useAgentProfiles";
import type { HostedConfig } from "../../model/hostedSession";
import { choiceOf, configOf } from "../../model/hostedConfig";
import { useHostedSession } from "../../store/useHostedSession";
import { useInstanceAgent } from "../../store/useInstanceAgent";
import "./hostedSession.css";

/**
 * The initial prompts typed and not saved yet, by session: a brick the host
 * unmounts (another tab, the drawer closed) gets its draft back.
 */
const drafts = new Map<string, string>();

/**
 * The configuration of an agent Session hébergée (CORE-125): its agent
 * (harness, model, effort) and its initial prompt, applied at its next
 * relaunch (the Opération `session` × `configure`). A shell has none, which
 * it says.
 */
export default function SessionConfig({ session: id }: { session: string }) {
  const { session, configure } = useHostedSession(id);
  const { profiles } = useAgentProfiles();
  const { catalog, inherited } = useInstanceAgent();
  const config = session?.config ?? {};
  const [draft, setDraftState] = useState<string | null>(() => drafts.get(id) ?? null);
  const setDraft = (next: string | null) => {
    if (next === null) drafts.delete(id);
    else drafts.set(id, next);
    setDraftState(next);
  };
  const [saving, setSaving] = useState(false);
  if (!session) return null;
  if (session.kind !== "agent") {
    return (
      <section className="hs-block" data-testid="hosted-session-config">
        <div className="hs-block__head">
          <span className="hs-block__title">Configuration</span>
        </div>
        <EmptyState icon={SquareTerminal} layout="row">
          A shell has no configuration
        </EmptyState>
      </section>
    );
  }

  const save = (next: HostedConfig) => {
    setSaving(true);
    configure(next)
      .then(() => setDraft(null))
      .catch((e: unknown) => toast(e instanceof Error ? e.message : String(e), "error", { title: "Configuration" }))
      .finally(() => setSaving(false));
  };
  const prompt = draft ?? config.prompt ?? "";
  const dirty = draft !== null && draft !== (config.prompt ?? "");
  return (
    <section className="hs-block" data-testid="hosted-session-config">
      <div className="hs-block__head">
        <span className="hs-block__title">Configuration</span>
        <span className="hs-block__hint">applies at next relaunch</span>
      </div>
      <AgentControl
        choice={choiceOf(config, inherited)}
        onChange={(choice) => save(configOf(choice, config.prompt, profiles, inherited))}
        profiles={profiles}
        catalog={catalog}
        inherited={inherited}
        label="Agent — hosted session"
        showLabel={false}
        testId="hosted-session-agent"
      />
      <div className="hs-block__head">
        <span className="hs-block__title">Initial prompt</span>
      </div>
      <TextArea
        mono
        className="hs-config__prompt"
        aria-label="Initial prompt"
        placeholder="None"
        value={prompt}
        disabled={saving}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && dirty) save({ ...config, prompt: draft || null });
          if (e.key === "Escape") setDraft(null);
        }}
        data-testid="hosted-session-prompt"
      />
      {dirty && (
        <div className="hs-config__save">
          <Button size="sm" tone="ghost" onClick={() => setDraft(null)}>
            Cancel
          </Button>
          <Button
            size="sm"
            tone="primary"
            disabled={saving}
            onClick={() => save({ ...config, prompt: draft || null })}
            data-testid="hosted-session-prompt-save"
          >
            Save
          </Button>
        </div>
      )}
    </section>
  );
}
