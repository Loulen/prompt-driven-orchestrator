import { useState } from "react";
import { FolderSearch, TriangleAlert } from "lucide-react";
import { Button, IconButton, TextInput } from "../../shared/ui/kit";
import FsExplorerModal from "../../workspace/components/FsExplorerModal";
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
    <div className="set-group" data-testid="staging-profile-image">
      <span className="d-caps set-group__head">Image</span>
      <div className="set-line">
        <select
          value={kind}
          onChange={(e) => chooseKind(e.target.value as "default" | "dockerfile" | "registry")}
          disabled={busy}
          aria-label="Image source"
          data-testid="staging-image-kind"
          className="d-input d-input--mono set-ctl-w"
        >
          <option value="default">default</option>
          <option value="dockerfile">dockerfile</option>
          <option value="registry">registry</option>
        </select>
        {kind === "default" && (
          <span
            className="set-faint set-mono set-ellipsis text-xs"
            title="Its tag is the SHA-256 of the bytes of the Dockerfile seeded there: edit that file to change the image"
            data-testid="staging-image-none"
          >
            ~/.pdo/sandbox/Dockerfile
          </span>
        )}
        {kind === "dockerfile" && (
          <>
            <TextInput
              mono
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="/path/to/Dockerfile.chrome-dev"
              aria-label="Dockerfile path"
              title="Self-contained (empty build context, no COPY); its bytes' SHA-256 tags the image, a suffix names it"
              data-testid="staging-image-path"
              autoComplete="off"
            />
            <IconButton
              icon={FolderSearch}
              label="Browse for a Dockerfile"
              size="sm"
              onClick={() => setPickerOpen(true)}
              data-testid="staging-image-browse"
            />
            <Button size="sm" onClick={() => pending && onWrite(pending)} disabled={!canSet} data-testid="staging-image-set">
              Set
            </Button>
          </>
        )}
        {kind === "registry" && (
          <>
            <TextInput
              mono
              value={ref}
              onChange={(e) => setRef(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && canSet && pending) onWrite(pending);
              }}
              placeholder="ghcr.io/owner/image:tag"
              aria-label="Image reference"
              data-testid="staging-image-ref"
              autoComplete="off"
            />
            <Button size="sm" onClick={() => pending && onWrite(pending)} disabled={!canSet} data-testid="staging-image-set">
              Set
            </Button>
          </>
        )}
      </div>
      {/* Load-bearing: the two properties an explicit ref LOSES. Without them the first
          failed pull looks like a bug of the daemon. */}
      {kind === "registry" && (
        <div
          className="set-warn"
          title="An explicit ref has no Dockerfile, hence no content hash and nothing to rebuild: a failed pull fails the Run, naming the ref. Nothing checks that the image contains the harness."
          data-testid="staging-image-ref-no-fallback"
        >
          <TriangleAlert size={13} strokeWidth={1.5} aria-hidden />
          <span>No local build to fall back on</span>
          <span className="set-faint set-ellipsis">a failed pull fails the Run</span>
        </div>
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
