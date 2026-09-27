import { useState } from "react";
import { Search } from "lucide-react";
import FsExplorerModal from "../../components/FsExplorerModal";
import { dockerfileStartDir } from "../model/paths";
import type { SandboxProfile } from "../model/sandbox";
import type { SandboxProfileImage } from "../../shared/generated/SandboxProfileImage";

/**
 * The profile's image source (#467, ADR-0031 §9): a three-way `<select>` (PDO's default /
 * Dockerfile / registry ref) plus the one field the chosen kind needs.
 *
 * Since #471 this is the ONLY place an image is chosen — the two instance-wide settings are gone,
 * and what they resolved to by default is a constant of the profile-defaults layer. So the copy
 * that used to live under the Settings field lives here now, in one sentence per kind: **the tag
 * is the SHA-256 of the Dockerfile's bytes.** That is what makes "edit the file to change the
 * image" comprehensible, and it belongs where the choice is made.
 *
 * Its own component for one reason: **the draft resets by remounting**. The parent keys it on the
 * profile name, so selecting another profile cannot leave a half-typed ref from the previous one
 * in the field — the `useEffect`-that-resets-state pattern is precisely what #385 was.
 *
 * The copy is the deliverable as much as the controls are. Two things have to be said about an
 * explicit ref, because nothing else in the product says them and each one is a real support
 * question:
 *
 * 1. **An explicit ref has no fallback.** No Dockerfile ⇒ no content hash ⇒ nothing to build if
 *    the pull fails, so the Run fails instead. That is the amendment to ADR-0030 pt 7.
 * 2. **PDO does not check the image contains `claude`.** Whoever supplies the ref owns that.
 */
export default function ProfileImageEditor({
  profile,
  busy,
  onWrite,
}: {
  profile: SandboxProfile;
  busy: boolean;
  onWrite: (image: SandboxProfileImage | null) => void;
}) {
  const [kind, setKind] = useState<"default" | "dockerfile" | "registry">(
    profile.image?.kind ?? "default",
  );
  const [path, setPath] = useState(
    profile.image?.kind === "dockerfile" ? profile.image.path : "",
  );
  const [ref, setRef] = useState(profile.image?.kind === "registry" ? profile.image.ref : "");
  const [pickerOpen, setPickerOpen] = useState(false);

  /** Switching to "default" IS the edit — there is no field to fill, so it writes straight
   *  away, consistent with every other control in this panel (nothing is batched). */
  const chooseKind = (next: "default" | "dockerfile" | "registry") => {
    setKind(next);
    if (next === "default" && profile.image !== null) onWrite(null);
  };

  const pending: SandboxProfileImage | null =
    kind === "dockerfile"
      ? { kind: "dockerfile", path: path.trim() }
      : kind === "registry"
        ? { kind: "registry", ref: ref.trim() }
        : null;
  const canSet =
    !busy &&
    ((pending?.kind === "dockerfile" && pending.path.length > 0) ||
      (pending?.kind === "registry" && pending.ref.length > 0));

  return (
    <div className="flex flex-col gap-1.5" data-testid="staging-profile-image">
      <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
        Image
      </span>
      <select
        value={kind}
        onChange={(e) => chooseKind(e.target.value as "default" | "dockerfile" | "registry")}
        disabled={busy}
        aria-label="Image source"
        data-testid="staging-image-kind"
        className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg focus:border-acc focus:outline-none disabled:opacity-40"
        style={{ fontSize: "11px" }}
      >
        <option value="default">default (PDO's own sandbox image)</option>
        <option value="dockerfile">dockerfile (build from a Dockerfile)</option>
        <option value="registry">registry (pull an explicit ref)</option>
      </select>

      {kind === "default" && (
        <div className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="staging-image-none">
          PDO's own image, whose tag is the SHA-256 of the bytes of the Dockerfile it seeded at{" "}
          <span className="font-mono">~/.pdo/sandbox/Dockerfile</span> — edit that file to change
          the image, or pick another kind here.
        </div>
      )}

      {kind === "dockerfile" && (
        <>
          <div className="flex items-center gap-2">
            <div className="relative min-w-0 flex-1">
              <input
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="/path/to/Dockerfile.chrome-dev"
                aria-label="Dockerfile path"
                data-testid="staging-image-path"
                className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 pr-8 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                style={{ fontSize: "11px" }}
                autoComplete="off"
              />
              <button
                type="button"
                onClick={() => setPickerOpen(true)}
                className="absolute inset-y-0 right-0 flex items-center px-2 text-fg-4 transition-colors hover:text-fg-2"
                title="Browse for a Dockerfile"
                aria-label="Browse for a Dockerfile"
                data-testid="staging-image-browse"
              >
                <Search size={12} />
              </button>
            </div>
            <button
              type="button"
              onClick={() => pending && onWrite(pending)}
              disabled={!canSet}
              data-testid="staging-image-set"
              className="shrink-0 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc disabled:opacity-40"
              style={{ fontSize: "11px" }}
            >
              Set
            </button>
          </div>
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Every Run on this profile builds (or pulls) the image whose tag is the SHA-256 of this
            file's bytes — editing the file changes the tag, hence a rebuild. It must be{" "}
            <strong>self-contained</strong>: the build context is deliberately empty, so no{" "}
            <span className="font-mono">COPY</span>. A filename like{" "}
            <span className="font-mono">Dockerfile.chrome-dev</span> also names the image (
            <span className="font-mono">pdo-sandbox-chrome-dev</span>).
          </div>
        </>
      )}

      {kind === "registry" && (
        <>
          <div className="flex items-center gap-2">
            <input
              value={ref}
              onChange={(e) => setRef(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && canSet && pending) onWrite(pending);
              }}
              placeholder="ghcr.io/owner/image:tag"
              aria-label="Image reference"
              data-testid="staging-image-ref"
              className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
              style={{ fontSize: "11px" }}
              autoComplete="off"
            />
            <button
              type="button"
              onClick={() => pending && onWrite(pending)}
              disabled={!canSet}
              data-testid="staging-image-set"
              className="shrink-0 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc disabled:opacity-40"
              style={{ fontSize: "11px" }}
            >
              Set
            </button>
          </div>
          {/* Load-bearing copy: the two properties an explicit ref LOSES. Without them the first
              failed pull looks like a PDO bug. */}
          <div
            className="text-st-await"
            style={{ fontSize: "10.5px" }}
            data-testid="staging-image-ref-no-fallback"
          >
            Pulled as-is, with <strong>no local build to fall back on</strong>: an explicit ref has
            no Dockerfile, so it has no content hash and nothing to rebuild — a failed pull{" "}
            <strong>fails the Run</strong>, naming the ref. PDO also cannot check that the image
            contains <span className="font-mono">claude</span>; that is on whoever supplies it.
          </div>
        </>
      )}

      {pickerOpen && (
        <FsExplorerModal
          mode="file"
          showHidden
          title="Choose a Dockerfile for this profile"
          confirmLabel="Use this Dockerfile"
          startPath={dockerfileStartDir(path)}
          onPick={(abs) => {
            setPath(abs);
            onWrite({ kind: "dockerfile", path: abs });
          }}
          onClose={() => setPickerOpen(false)}
        />
      )}
    </div>
  );
}
