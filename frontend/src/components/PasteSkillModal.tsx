import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ClipboardPaste, Folder, X } from "lucide-react";
import { ApiError, createSkill } from "../api";
import type { Skill, SkillFolder } from "../types";
import { validateSkillMd, type CheckId, type CheckState } from "../lib/skillMd";
import { folderPathLabel } from "../lib/skillTree";

interface Props {
  folders: SkillFolder[];
  /** Current bank labels, for the live `unique` check. */
  existingNames: string[];
  /** Folder pre-selected when the popup opened (the tree's selection). */
  initialFolderId: string | null;
  onClose: () => void;
  onCreated: (skill: Skill) => void | Promise<void>;
}

/** Map a daemon refusal `code` to the check it should light red. */
const CODE_TO_CHECK: Record<string, CheckId> = {
  no_frontmatter: "frontmatter",
  malformed_frontmatter: "frontmatter",
  missing_name: "name",
  name_not_kebab_case: "name",
  empty_label: "name",
  missing_description: "description",
  empty_body: "body",
  duplicate_name: "unique",
};

/**
 * "New skill from SKILL.md" (#668, FP steps 2-3): paste the text, five checks
 * re-run on every keystroke, the preview card is exactly the tree row it will
 * become, and Create enables only when all five pass. A refusal — local or a
 * daemon 400/409 — is shown **in place** (red check, red textarea border, callout
 * with reason and consequence), never as a modal on top of this modal.
 *
 * Nothing touches disk until Create: the daemon validates again and writes the
 * folder only after the row is indexed. Same `z-[60]` layer as `FsExplorerModal`
 * (they are never open at once).
 */
export default function PasteSkillModal({
  folders,
  existingNames,
  initialFolderId,
  onClose,
  onCreated,
}: Props) {
  const [text, setText] = useState("");
  const [folderId, setFolderId] = useState<string | null>(initialFolderId);
  const [submitting, setSubmitting] = useState(false);
  const [serverError, setServerError] = useState<{ code: string; message: string } | null>(null);
  /** The text a 409 was returned for; the `unique` check stays red until it changes. */
  const [duplicateFor, setDuplicateFor] = useState<string | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  const serverDuplicate = duplicateFor !== null && duplicateFor === text;
  const validation = useMemo(
    () => validateSkillMd(text, existingNames, serverDuplicate),
    [text, existingNames, serverDuplicate],
  );

  // A daemon 400 for the CURRENT text overrides the check it names; any edit clears it.
  const serverCheck: CheckId | null =
    serverError && CODE_TO_CHECK[serverError.code] ? CODE_TO_CHECK[serverError.code] : null;
  const checks = validation.checks.map((check) =>
    serverCheck === check.id && check.state === "pass"
      ? { ...check, state: "fail" as CheckState }
      : check,
  );
  const failing = checks.some((check) => check.state === "fail");
  const canCreate = validation.valid && !serverError && !submitting && text.trim() !== "";
  const reason = serverError?.message ?? validation.reason;

  const requestClose = () => {
    if (text.trim() !== "" && !confirmDiscard) {
      setConfirmDiscard(true);
      return;
    }
    onClose();
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        requestClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text, confirmDiscard]);

  const submit = async () => {
    if (!canCreate) return;
    setSubmitting(true);
    setServerError(null);
    try {
      const skill = await createSkill({ content: text, folder_id: folderId });
      await onCreated(skill);
      onClose();
    } catch (cause) {
      if (cause instanceof ApiError) {
        const body = cause.body as { code?: unknown } | null;
        const code = typeof body?.code === "string" ? body.code : "";
        if (cause.status === 409 || code === "duplicate_name") {
          setDuplicateFor(text);
        } else {
          setServerError({ code, message: cause.message });
        }
      } else {
        setServerError({
          code: "",
          message: cause instanceof Error ? cause.message : "Failed to create the skill",
        });
      }
    } finally {
      setSubmitting(false);
    }
  };

  const preview = validation.parsed;
  const previewName = preview.name ?? "(name)";
  const previewDescription = preview.description ?? "(description)";
  const selectedFolder = folders.find((folder) => folder.id === folderId) ?? null;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(event) => {
        event.stopPropagation();
        requestClose();
      }}
      data-testid="paste-skill-backdrop"
    >
      <div
        className="flex w-[1020px] max-w-[96vw] max-h-[88vh] flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label="New skill from SKILL.md"
        data-testid="paste-skill-modal"
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            <ClipboardPaste size={14} className="text-fg-3" />
            New skill from SKILL.md
          </h3>
          <button
            type="button"
            onClick={requestClose}
            aria-label="Close paste popup"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 gap-4 p-4">
          {/* Left: the pasted text */}
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <textarea
              ref={textareaRef}
              value={text}
              onChange={(event) => {
                setText(event.target.value);
                setServerError(null);
                setConfirmDiscard(false);
              }}
              onPaste={() => setConfirmDiscard(false)}
              spellCheck={false}
              placeholder={"---\nname: my-skill\ndescription: What it does, when to use it.\n---\n\n# My skill\n\nInstructions…"}
              aria-label="SKILL.md text"
              aria-invalid={failing || undefined}
              data-testid="paste-skill-text"
              className={`min-h-[340px] flex-1 resize-none rounded-md border bg-bg-1 p-3 font-mono text-fg outline-none transition-colors ${
                failing ? "border-st-failed" : text.trim() && validation.valid ? "border-acc/60" : "border-line-strong focus:border-acc"
              }`}
              style={{ fontSize: "11.5px", lineHeight: 1.55 }}
            />
            <div
              className="rounded-md border border-dashed border-line px-3 py-2 text-fg-4"
              style={{ fontSize: "10.5px" }}
            >
              SKILL.md only here · to bring reference files along, import the skill's folder from a source
            </div>
          </div>

          {/* Right: checks, preview, folder */}
          <div className="flex w-[330px] shrink-0 flex-col gap-4 overflow-y-auto">
            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Checks
              </h4>
              <ul className="flex flex-col gap-1.5" data-testid="paste-skill-checks">
                {checks.map((check) => (
                  <li
                    key={check.id}
                    className="flex items-center gap-2 text-fg-2"
                    style={{ fontSize: "11.5px" }}
                    data-testid={`check-${check.id}`}
                    data-state={check.state}
                  >
                    <CheckDot state={check.state} />
                    <span className={check.state === "fail" ? "text-fg" : check.state === "pending" ? "text-fg-4" : ""}>
                      {check.label}
                    </span>
                  </li>
                ))}
              </ul>
            </section>

            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Will appear as
              </h4>
              <div
                className={`rounded-md border border-line bg-bg-3 px-3 py-2.5 ${validation.valid ? "" : "opacity-70"}`}
                data-testid="paste-skill-preview"
              >
                <div className={`font-semibold ${preview.name ? "text-fg" : "text-fg-4"}`} style={{ fontSize: "12.5px" }}>
                  {previewName}
                </div>
                <div className={`mt-0.5 ${preview.description ? "text-fg-3" : "text-fg-4"}`} style={{ fontSize: "11px" }}>
                  {previewDescription}
                </div>
              </div>
              {reason && (
                <div
                  role="alert"
                  className="mt-2 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
                  style={{ fontSize: "11px" }}
                  data-testid="paste-skill-reason"
                >
                  <strong className="text-fg">
                    {checks.find((c) => c.id === "unique")?.state === "fail" && !failingOther(checks)
                      ? "Name already taken."
                      : "Cannot create."}
                  </strong>{" "}
                  {reason}
                </div>
              )}
            </section>

            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Folder
              </h4>
              <label className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5">
                <Folder size={12} className="shrink-0 text-fg-3" />
                <select
                  value={folderId ?? ""}
                  onChange={(event) => setFolderId(event.target.value || null)}
                  aria-label="Folder"
                  data-testid="paste-skill-folder"
                  className="w-full bg-transparent text-fg outline-none"
                  style={{ fontSize: "11.5px" }}
                >
                  <option value="">Root of the bank</option>
                  {folders.map((folder) => (
                    <option key={folder.id} value={folder.id}>
                      {folderPathLabel(folder.id, folders)}
                    </option>
                  ))}
                </select>
              </label>
              {selectedFolder === null && folderId !== null && (
                <p className="mt-1 text-st-blocked" style={{ fontSize: "10px" }}>
                  That folder no longer exists; the skill will land at the root.
                </p>
              )}
            </section>
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Validated as you type · nothing touches disk until Create
          </span>
          <div className="flex items-center gap-2">
            {confirmDiscard ? (
              <>
                <span className="text-fg-2" style={{ fontSize: "11px" }} data-testid="paste-skill-discard-prompt">
                  Discard the pasted text?
                </span>
                <button
                  type="button"
                  onClick={() => setConfirmDiscard(false)}
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Keep editing
                </button>
                <button
                  type="button"
                  onClick={onClose}
                  data-testid="paste-skill-discard"
                  className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-white hover:opacity-90"
                  style={{ fontSize: "11.5px" }}
                >
                  Discard
                </button>
              </>
            ) : (
              <>
                <button
                  type="button"
                  onClick={requestClose}
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={() => void submit()}
                  disabled={!canCreate}
                  data-testid="paste-skill-create"
                  className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
                  style={{ fontSize: "11.5px" }}
                >
                  {submitting ? "Creating…" : "Create skill"}
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function failingOther(checks: { id: CheckId; state: CheckState }[]): boolean {
  return checks.some((check) => check.id !== "unique" && check.state === "fail");
}

function CheckDot({ state }: { state: CheckState }) {
  if (state === "pass") {
    return (
      <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-acc/20 text-acc">
        <Check size={9} strokeWidth={3} />
      </span>
    );
  }
  if (state === "fail") {
    return (
      <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-st-failed/20 text-st-failed">
        <X size={9} strokeWidth={3} />
      </span>
    );
  }
  if (state === "warn") {
    return <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-st-await/20 text-st-await font-bold" style={{ fontSize: 9 }}>!</span>;
  }
  return <span className="h-3.5 w-3.5 rounded-full border border-line-strong" />;
}
