/**
 * Settings › Sandbox & worktrees › Worktree provisioning: the instance's rules as rows, one
 * per mode, written at once like every panel with its own persistence (no Save of its own,
 * never part of the instance form). A repository to resolve against previews the rules:
 * matched paths per pattern, conflicts at the row end, the resolved plan under the rows.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type { SettingsSection } from "../../model/settingsCategories";
import { fetchInstanceProvisioning, previewProvisioning, saveInstanceProvisioning } from "../../../workspace/api/provisioning";
import { EMPTY_PROVISIONING_RULES, ruleLines } from "../../../workspace/model/provisioning";
import { LEVELS_HINT, MODES, SUBTITLE } from "../../../workspace/model/provisioningCopy";
import ProvisioningResolvedPlan from "../../../workspace/components/ProvisioningResolvedPlan";
import type { ProvisioningMode } from "../../../shared/generated/ProvisioningMode";
import type { ProvisioningPlan } from "../../../shared/generated/ProvisioningPlan";
import type { ProvisioningRules } from "../../../shared/generated/ProvisioningRules";
import { TextArea, TextInput, toast } from "../../../shared/ui/kit";
import { RowError, SettingRow } from "./fields";
import Section, { SectionExtra } from "./Section";

/** The pause after the last keystroke before the rules are written. */
export const PROVISIONING_SAVE_DELAY_MS = 600;

type SaveState = { status: "idle" | "saving" | "saved" } | { status: "failed"; message: string };

const label = (mode: ProvisioningMode) => `${mode[0].toUpperCase()}${mode.slice(1)}`;

export default function ProvisioningRows({ section }: { section: SettingsSection }) {
  // The rules as the textareas hold them (the raw text, blank lines included), the rules
  // the daemon last stored, and the last rules sent to it.
  const [texts, setTexts] = useState<Record<ProvisioningMode, string> | null>(null);
  const stored = useRef<string>("");
  const sent = useRef<string>("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [repository, setRepository] = useState("");
  const [preview, setPreview] = useState<{ key: string; plan: ProvisioningPlan } | null>(null);
  const [previewFailure, setPreviewFailure] = useState<{ key: string; message: string } | null>(null);
  const [save, setSave] = useState<SaveState>({ status: "idle" });

  const rules = useMemo<ProvisioningRules>(
    () =>
      texts
        ? { copy: ruleLines(texts.copy), hardlink: ruleLines(texts.hardlink), symlink: ruleLines(texts.symlink) }
        : EMPTY_PROVISIONING_RULES,
    [texts],
  );
  const serialized = JSON.stringify(rules);

  useEffect(() => {
    let cancelled = false;
    fetchInstanceProvisioning()
      .then((loaded) => {
        if (cancelled) return;
        stored.current = JSON.stringify(loaded);
        sent.current = stored.current;
        setTexts({ copy: loaded.copy.join("\n"), hardlink: loaded.hardlink.join("\n"), symlink: loaded.symlink.join("\n") });
      })
      .catch((error: unknown) => {
        if (!cancelled) setLoadError(error instanceof Error ? error.message : "Failed to load rules");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // The preview, against the repository typed above the rules; none without one. Each
  // result remembers what it was computed for, so a stale one never vouches for an edit.
  const previewing = repository.trim() !== "";
  const previewKey = `${repository}\n${serialized}`;
  const plan = previewing ? (preview?.plan ?? null) : null;
  const previewError = previewing && previewFailure?.key === previewKey ? previewFailure.message : null;
  useEffect(() => {
    if (!previewing) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      previewProvisioning(repository, "instance", JSON.parse(serialized) as ProvisioningRules, [])
        .then((next) => {
          if (cancelled) return;
          setPreview({ key: previewKey, plan: next });
          setPreviewFailure(null);
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          setPreview(null);
          setPreviewFailure({ key: previewKey, message: error instanceof Error ? error.message : "Preview failed" });
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [previewing, repository, serialized, previewKey]);

  // Under a preview, the rules are written only once the preview of these very rules is
  // back without a conflict.
  const blocked =
    previewing &&
    (preview?.key !== previewKey ||
      !!previewError ||
      preview.plan.conflicts.some((c) => c.scope === "instance"));

  // Written at once: a pause after the last edit, one write after the other, and only the
  // latest speaks. An edit still waiting when the section goes (Settings closed) is written
  // on the way out; one held by a conflict is not, and a toast says so.
  const pending = useRef<string | null>(null);
  const heldBack = useRef(false);
  const chain = useRef<Promise<void>>(Promise.resolve());
  const writes = useRef(0);
  const mounted = useRef(true);
  const write = (payload: string) => {
    pending.current = null;
    sent.current = payload;
    const seq = ++writes.current;
    if (mounted.current) setSave({ status: "saving" });
    const run = chain.current.then(() => saveInstanceProvisioning(JSON.parse(payload) as ProvisioningRules));
    chain.current = run.then(
      () => undefined,
      () => undefined,
    );
    run
      .then((saved) => {
        stored.current = JSON.stringify(saved);
        if (seq === writes.current && mounted.current) setSave({ status: "saved" });
      })
      .catch((error: unknown) => {
        if (seq !== writes.current) return;
        // The next edit tries again from what the daemon holds.
        sent.current = stored.current;
        if (mounted.current) {
          setSave({ status: "failed", message: error instanceof Error ? error.message : "Failed to save rules" });
        }
      });
  };
  const writeRef = useRef(write);
  useEffect(() => {
    writeRef.current = write;
  });
  useEffect(() => {
    const changed = texts != null && serialized !== sent.current;
    heldBack.current = changed && blocked;
    if (!changed || blocked) {
      pending.current = null;
      return;
    }
    pending.current = serialized;
    const timer = window.setTimeout(() => writeRef.current(serialized), PROVISIONING_SAVE_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [serialized, blocked, texts]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (pending.current != null) writeRef.current(pending.current);
      else if (heldBack.current) toast("Provisioning not saved: fix the conflict first", "warning");
    };
  }, []);

  const status =
    save.status === "saving" ? (
      <span className="set-faint text-xs">Saving…</span>
    ) : save.status === "saved" ? (
      <span className="set-faint text-xs" data-testid="provisioning-saved">
        Saved
      </span>
    ) : save.status === "failed" ? (
      <RowError title={save.message} testId="provisioning-save-error">
        Not saved
      </RowError>
    ) : null;

  const counts = (mode: ProvisioningMode) =>
    (plan?.rules ?? [])
      .filter((rule) => rule.scope === "instance" && rule.mode === mode)
      .reduce((total, rule) => total + rule.paths.length + rule.excluded_paths.length, 0);


  return (
    <Section
      section={section}
      tools={
        <span role="status" aria-live="polite">
          {status}
        </span>
      }
    >
      <SettingRow
        label="Resolve against"
        help={`${SUBTITLE}\nA repository to preview the rules on; nothing is stored`}
        keywords="provisioning preview repository"
        kind="wide"
        htmlFor="setting-provisioning-repository"
        testId="setting-row-provisioning-repository"
        note={previewError ?? undefined}
        noteTone="failed"
      >
        <TextInput
          id="setting-provisioning-repository"
          mono
          className="set-grow"
          value={repository}
          onChange={(event) => setRepository(event.target.value)}
          placeholder="/absolute/path/to/repository"
        />
      </SettingRow>
      {loadError && (
        <div className="set-row__note set-row__note--failed" role="alert">
          <span className="set-ellipsis" title={loadError}>
            {loadError}
          </span>
        </div>
      )}
      {MODES.map(({ key, hint }) => {
        const conflict = plan?.conflicts.find((c) => c.scope === "instance" && c.modes.includes(key));
        return (
          <SettingRow
            key={key}
            label={label(key)}
            help={`${hint}\nOne pattern per line; !pattern excludes.\n${LEVELS_HINT}`}
            keywords={`provisioning ${key} gitignore .env`}
            kind="wide"
            top
            htmlFor={`setting-provisioning-${key}`}
            testId={`setting-row-provisioning-${key}`}
          >
            <TextArea
              id={`setting-provisioning-${key}`}
              aria-label={`${label(key)} patterns`}
              mono
              rows={2}
              className="set-grow"
              invalid={!!conflict}
              disabled={texts == null}
              value={texts?.[key] ?? ""}
              onChange={(event) => {
                const value = event.target.value;
                setTexts((current) => (current ? { ...current, [key]: value } : current));
                setSave((current) => (current.status === "saved" ? { status: "idle" } : current));
              }}
              placeholder={key === "copy" ? ".env" : key === "symlink" ? "node_modules" : undefined}
            />
            {conflict && (
              <RowError title={`${conflict.relative_path}: ${conflict.modes.join(" and ")}; keep one`}>
                conflict
              </RowError>
            )}
            {plan && !conflict && (
              <span className="set-faint set-mono text-xs" title="Paths matched in the repository">
                {counts(key)}
              </span>
            )}
          </SettingRow>
        );
      })}
      {plan && (
        <SectionExtra testId="provisioning-resolved">
          <div className="text-xs">
            <ProvisioningResolvedPlan plan={plan} frozen={false} />
          </div>
        </SectionExtra>
      )}
    </Section>
  );
}
