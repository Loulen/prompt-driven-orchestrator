import { useEffect, useRef, useState } from "react";
import { Save } from "lucide-react";
import type { PipelineVersionRef } from "../../shared/generated/PipelineVersionRef";
import { Button, Kbd, Modal, Segmented, TextInput, toast } from "../../shared/ui/kit";
import type { SaveResult } from "../api/pipelines";
import type { SaveAs } from "../api/versions";
import { defaultTitle } from "../model/versions";
import "./inspect.css";

/** The three outcomes of a Save; they exclude each other. */
type SaveKind = "version" | "keep" | "replace";

/** What the button says: « Save v8 », « Save v8, keep v7 », « Replace v7 ». */
function saveActionLabel(kind: SaveKind, last: PipelineVersionRef): string {
  const n = last.number;
  if (kind === "version") return `Save v${n + 1}`;
  if (kind === "keep") return `Save v${n + 1}, keep v${n}`;
  return `Replace v${n}`;
}

/** The pipeline the last version is kept as: `<name> (vN)`. */
function keptName(pipelineName: string, number: number): string {
  return `${pipelineName} (${defaultTitle(number)})`;
}

function saveAsOf(kind: SaveKind, title: string): SaveAs {
  if (kind === "replace") return { new_version: false, keep_old: false };
  return { new_version: true, title, keep_old: kind === "keep" };
}

/** What the daemon did, by the version it numbered: another Save may have come first. */
function savedMessage(pipelineName: string, saveAs: SaveAs, saved: PipelineVersionRef, kept: boolean): string {
  const name = `${pipelineName} ${defaultTitle(saved.number)}`;
  if (!saveAs.new_version) return `Replaced ${name}`;
  // Kept: the pipeline's name once, so a long one leaves the kept name in view.
  return kept ? `Saved ${defaultTitle(saved.number)} · kept ${keptName(pipelineName, saved.number - 1)}` : `Saved ${name}`;
}

/**
 * « Save <pipeline> » of a shared draft (ADR-0088): one choice among three,
 * the button names the outcome. A new version `vN+1`, titled `vN+1` unless
 * renamed; the same, with `vN` also kept as a pipeline of its own; or the
 * draft replaces `vN` under its number.
 */
export default function SaveVersionModal({
  pipelineName,
  last,
  onSave,
  onClose,
}: {
  pipelineName: string;
  /** The last version: the file, what a replacement replaces. */
  last: PipelineVersionRef;
  /** Resolves what the daemon saved; nothing when the save failed. */
  onSave: (saveAs: SaveAs) => Promise<SaveResult | undefined>;
  onClose: () => void;
}) {
  const n = last.number;
  const [kind, setKind] = useState<SaveKind>("version");
  const [title, setTitle] = useState(defaultTitle(n + 1));
  const [saving, setSaving] = useState(false);
  // Enter and Ctrl+Enter twice before a render must not save twice.
  const inFlight = useRef(false);
  const titleRef = useRef<HTMLInputElement>(null);

  // The default title is selected: typing replaces it.
  useEffect(() => {
    titleRef.current?.select();
  }, []);

  const lastName = defaultTitle(n);
  const lastTitle = last.title.trim() !== "" && last.title !== lastName ? last.title : null;
  const replace = kind === "replace";
  const action = saveActionLabel(kind, last);

  const submit = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setSaving(true);
    const saveAs = saveAsOf(kind, title);
    const result = await onSave(saveAs);
    inFlight.current = false;
    setSaving(false);
    if (result?.version) toast(savedMessage(pipelineName, saveAs, result.version, result.kept != null), "success");
  };

  return (
    <Modal
      size="s"
      title={
        <>
          Save <span className="font-mono">{pipelineName}</span>
        </>
      }
      titleText={`Save ${pipelineName}`}
      onClose={onClose}
      onSubmit={() => void submit()}
      data-testid="save-version-modal"
      actions={
        <>
          <Button onClick={onClose} data-testid="save-version-cancel">
            Cancel
          </Button>
          <Button
            tone={replace ? "danger" : "primary"}
            icon={Save}
            disabled={saving}
            title={`${action} (Ctrl+Enter)`}
            onClick={() => void submit()}
            data-testid="save-version-confirm"
          >
            {action}
            <Kbd>Ctrl ↵</Kbd>
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-2 text-sm" data-testid="save-version-body">
        <div className="flex *:min-w-0 *:flex-1 [&_[role=radio]]:min-w-0 [&_[role=radio]]:flex-1 [&_[role=radio]]:justify-center [&_[role=radio]>span]:truncate">
          <Segmented<SaveKind>
            label="Save as"
            value={kind}
            onChange={setKind}
            items={[
              {
                value: "version",
                label: `New version v${n + 1}`,
                title: "New version",
              },
              {
                value: "keep",
                label: `Keep v${n} apart`,
                title: `New version, keep v${n} as a pipeline`,
              },
              {
                value: "replace",
                label: `Replace v${n}`,
                title: `Replace v${n}`,
              },
            ]}
          />
        </div>
        <label className="grid grid-cols-[40px_minmax(0,1fr)] items-center gap-x-1.5">
          <span className="text-text-2">Title</span>
          <TextInput
            ref={titleRef}
            mono
            data-autofocus=""
            aria-label="Version title"
            spellCheck={false}
            value={replace ? (lastTitle ?? lastName) : title}
            disabled={replace}
            // A Replace keeps the version's title: the field only shows it.
            className={replace ? "ei-ro" : undefined}
            title={replace ? `Replace keeps the title of ${lastName}` : undefined}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
                e.preventDefault();
                void submit();
              }
            }}
            data-testid="save-version-title-input"
          />
        </label>
        <p
          className={`m-0 flex min-h-5 min-w-0 items-center gap-1 ${replace ? "text-st-await" : "text-text-2"}`}
          data-testid="save-version-effect"
        >
          {kind === "version" && <>{lastName} stays in Versions</>}
          {kind === "keep" && (
            <>
              <span className="flex-none">{lastName} also kept as</span>{" "}
              <code className="d-code min-w-0 truncate" title={keptName(pipelineName, n)}>
                {keptName(pipelineName, n)}
              </code>
            </>
          )}
          {replace && (
            <span className="min-w-0 truncate" title={lastTitle ?? undefined}>
              Overwrites {lastName}
              {lastTitle && <> « {lastTitle} »</>}, same number
            </span>
          )}
        </p>
      </div>
    </Modal>
  );
}
