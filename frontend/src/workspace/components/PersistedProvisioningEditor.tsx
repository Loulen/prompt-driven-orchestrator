import { useEffect, useState } from "react";
import { fetchInstanceProvisioning, fetchProjectProvisioning, saveInstanceProvisioning, saveProjectProvisioning } from "../api/provisioning";
import { EMPTY_PROVISIONING_RULES } from "../model/provisioning";
import type { ProvisioningRules } from "../../shared/generated/ProvisioningRules";
import ProvisioningRulesEditor from "./ProvisioningRulesEditor";
import { Button } from "../../shared/ui/kit";
import { tildify, untildify } from "../../lib/homePath";

export default function PersistedProvisioningEditor({
  scope,
  projectId,
  initialRepository = "",
  defaultExpanded = false,
  home = null,
}: {
  scope: "instance" | "project";
  projectId?: string;
  initialRepository?: string;
  defaultExpanded?: boolean;
  /** The daemon's home, to read and type the repository as `~/code/…`. */
  home?: string | null;
}) {
  // The field as typed; `null` until it is, so a home read after the opening
  // still shortens the initial repository.
  const [typed, setTyped] = useState<string | null>(null);
  const repository = typed === null ? initialRepository : untildify(typed, home);
  const [rules, setRules] = useState<ProvisioningRules>(EMPTY_PROVISIONING_RULES);
  const [valid, setValid] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const request =
      scope === "instance"
        ? fetchInstanceProvisioning()
        : projectId
          ? fetchProjectProvisioning(projectId)
          : Promise.resolve(EMPTY_PROVISIONING_RULES);
    request
      .then((loaded) => {
        if (!cancelled) setRules(loaded);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setMessage(error instanceof Error ? error.message : "Failed to load rules");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [scope, projectId]);

  async function save() {
    if (!valid || (scope === "project" && !projectId)) return;
    setSaving(true);
    setMessage(null);
    try {
      if (scope === "instance") await saveInstanceProvisioning(rules);
      else await saveProjectProvisioning(projectId!, rules);
      setMessage("Provisioning rules saved.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Failed to save rules");
    } finally {
      setSaving(false);
    }
  }

  return (
    <ProvisioningRulesEditor
      level={scope}
      repository={repository}
      rules={rules}
      onChange={setRules}
      onValidityChange={setValid}
      projectId={scope === "project" ? (projectId ?? null) : null}
      defaultExpanded={defaultExpanded}
      home={home}
      leading={
        <label className="ln-line ln-prov__against">
          <span className="ln-faint">Resolve against</span>
          <input
            value={typed ?? tildify(initialRepository, home)}
            onChange={(event) => setTyped(event.target.value)}
            title={repository || undefined}
            placeholder="/absolute/path/to/repository"
            spellCheck={false}
            className="d-input d-input--sm d-input--mono ln-grow"
          />
        </label>
      }
      trailing={
        <div className="ln-line">
          <span className={message?.includes("Failed") ? "ln-err" : "ln-faint"}>{message}</span>
          <span className="ln-grow" />
          <Button
            size="sm"
            tone="primary"
            onClick={save}
            disabled={!valid || saving || (scope === "project" && !projectId)}
          >
            {saving ? "Saving…" : "Save provisioning"}
          </Button>
        </div>
      }
    />
  );
}
