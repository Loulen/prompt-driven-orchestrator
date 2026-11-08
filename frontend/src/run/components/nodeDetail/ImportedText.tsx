import { useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";
import { importedFilePhrase } from "../../model/nodeImport";
import { copyToClipboard } from "../../../shared/lib/clipboard";

/** « Fichier importé en cours de Run »: the ready-to-use text of an import, one
 *  phrase per file, and its copy icon. `text` is what the icon copies. */
export default function ImportedText({ paths, text }: { paths: string[]; text: string }) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  // The feedback goes back to idle after a beat. The timer belongs to the
  // feedback it resets, and dies with the component: a timer left running past
  // an unmount would set state on a tree that no longer exists.
  useEffect(() => {
    if (copyState === "idle") return;
    const timer = window.setTimeout(() => setCopyState("idle"), 1500);
    return () => window.clearTimeout(timer);
  }, [copyState]);

  const copyText = async () => {
    const ok = await copyToClipboard(text);
    setCopyState(ok ? "copied" : "failed");
  };

  return (
    <div className="relative rounded-md border border-line-strong bg-surface-0 p-3 pr-10">
      <div
        className="flex flex-col gap-1.5 break-words font-mono text-text text-xs"
        data-testid="import-text"
      >
        {paths.map((path) => (
          <ImportedFilePhrase key={path} path={path} />
        ))}
      </div>
      <button
        type="button"
        onClick={() => void copyText()}
        aria-label="Copy text"
        title={copyState === "copied" ? "Copied" : copyState === "failed" ? "Copy failed" : "Copy text"}
        className="absolute right-2 top-2 grid h-6 w-6 place-items-center rounded text-text-3 hover:bg-active hover:text-text"
        data-testid="import-copy"
        data-feedback={copyState === "idle" ? undefined : copyState}
      >
        {copyState === "copied" ? <Check size={13} /> : <Copy size={13} />}
      </button>
    </div>
  );
}

/** One file's phrase, its path allowed to wrap after each `/` rather than in
 *  the middle of a name. The copied text is the same phrase, without the
 *  break hints. */
function ImportedFilePhrase({ path }: { path: string }) {
  const [before, after] = importedFilePhrase(path).split(path);
  const parts = path.split("/");
  return (
    <p data-testid="import-phrase">
      {before}
      {parts.map((part, i) => (
        <span key={i}>
          {part}
          {i < parts.length - 1 && (
            <>
              /<wbr />
            </>
          )}
        </span>
      ))}
      {after}
    </p>
  );
}
