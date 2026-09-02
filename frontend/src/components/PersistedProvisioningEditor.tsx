import { useEffect, useState } from "react";
import {
  fetchInstanceProvisioning,
  fetchProjectProvisioning,
  saveInstanceProvisioning,
  saveProjectProvisioning,
} from "../api";
import { EMPTY_PROVISIONING_RULES } from "../lib/provisioning";
import type { ProvisioningRules } from "../types";
import ProvisioningRulesEditor from "./ProvisioningRulesEditor";

export default function PersistedProvisioningEditor({
  scope,
  projectId,
  initialRepository = "",
}: {
  scope: "instance" | "project";
  projectId?: string;
  initialRepository?: string;
}) {
  const [repository, setRepository] = useState(initialRepository);
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
    <div className="space-y-2">
      <label className="block text-fg-3" style={{ fontSize: 10 }}>
        Resolve against
        <input
          value={repository}
          onChange={(event) => setRepository(event.target.value)}
          placeholder="/absolute/path/to/repository"
          className="mt-1 w-full rounded border border-line-strong bg-bg-3 px-2 py-1 font-mono text-fg outline-none focus:border-acc"
        />
      </label>
      <ProvisioningRulesEditor
        level={scope}
        repository={repository}
        rules={rules}
        onChange={setRules}
        onValidityChange={setValid}
      />
      <div className="flex items-center justify-between">
        <span className={message?.includes("Failed") ? "text-st-failed" : "text-fg-4"}>
          {message}
        </span>
        <button
          type="button"
          onClick={save}
          disabled={!valid || saving || (scope === "project" && !projectId)}
          className="rounded bg-acc px-2.5 py-1 font-medium text-[#04140d] disabled:opacity-40"
        >
          {saving ? "Saving…" : "Save provisioning"}
        </button>
      </div>
    </div>
  );
}
