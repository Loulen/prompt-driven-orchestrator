import { useCallback, useEffect, useState } from "react";
import { Plus, Search, Trash2, X } from "lucide-react";
import FsExplorerModal from "./FsExplorerModal";
import {
  deleteSandboxProfile,
  fetchSandboxProfileReferents,
  fetchSandboxProfiles,
  saveSandboxProfile,
} from "../api";
import type {
  SandboxProfile,
  SandboxProfileEntry,
  SandboxProfileImage,
  SandboxProfileReferents,
} from "../types";


/** Open-at directory for the Dockerfile picker (#431/#467): the PARENT of the current
 *  absolute path, so the explorer lands where the file lives rather than at `$HOME`.
 *  Anything non-absolute (or absent) → undefined, i.e. the daemon's default chain. The
 *  daemon also clamps a file path to its parent, but resolving it here keeps the very
 *  first request honest about what it is asking for. */
function dockerfileStartDir(path: string | null): string | undefined {
  const trimmed = (path ?? "").trim();
  if (!trimmed.startsWith("/")) return undefined;
  const lastSlash = trimmed.lastIndexOf("/");
  return lastSlash <= 0 ? "/" : trimmed.slice(0, lastSlash);
}

/** Which tier turn-end auto-completion comes from (#469). Like the enum knobs there IS
 *  a built-in default (`off`), and both directions of a save are a stored decision — so

/**
 * Turn an absolute path from {@link FsExplorerModal} into a `$HOME`-relative profile
 * entry. `null` when it does not live under `$HOME` (or `$HOME` is unknown), so the caller
 * can say so inline instead of firing a `PUT` the daemon will 400.
 *
 * A pure helper next to {@link dockerfileStartDir}, and the reason `GET /settings` grew a
 * `home` field: `onPick` yields an ABSOLUTE path, an entry is RELATIVE, and no endpoint
 * exposed `$HOME` before this. Do NOT derive it from any other path the payload happens to
 * carry — `home` lives behind the `sandbox_home_override` seam, and the daemon revalidates
 * at the edge regardless.
 */
// Exported for its own unit test. Precedent: `RUN_INTENT` in `NewRunModal.tsx` — a pure,
// render-free value whose Fast-Refresh cost is nil, opted out one line at a time rather
// than moved to a module of its own.
// eslint-disable-next-line react-refresh/only-export-components
export function relativiseToHome(abs: string, home: string | null): string | null {
  if (!home) return null;
  const h = home.replace(/\/+$/, "");
  if (abs === h) return null; // `$HOME` itself is not an entry
  const prefix = `${h}/`;
  if (!abs.startsWith(prefix)) return null;
  const rel = abs.slice(prefix.length).replace(/\/+$/, "");
  return rel.length > 0 ? rel : null;
}

interface PanelProps {
  /** Host `$HOME`, from `GET /settings`. `null` disables the pickers' relativisation. */
  home: string | null;
  /** Drawer/dialog host: renders a Done footer. Absent when mounted inline (#691). */
  onDone?: () => void;
  /** Called after every successful write so the parent can refetch `GET /settings`. */
  onChanged: () => void;
}

/**
 * The staging-profile editor: a list of profiles on top, the selected one's entries below.
 *
 * Two things it deliberately does NOT do:
 * - it never computes a warning or a size itself. The disk-cost note, the `sensitive` flag
 *   and the floor block all arrive from the daemon (#373 discipline: a client-side tag
 *   re-opens exactly the drift that cost us before). A real recursive walk of every
 *   `node_modules` under `plugins/` is seconds of IO anyway.
 * - it never batches. Each toggle / add / remove is its own `PUT`, which is why the footer
 *   says **Done** and not Save — profiles are their own REST resource, not part of the
 *   grouped `PUT /settings`.
 */

export default function StagingProfilesPanel({ home, onDone, onChanged }: PanelProps) {
  const [profiles, setProfiles] = useState<SandboxProfile[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [pickerMode, setPickerMode] = useState<"file" | "dir" | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<SandboxProfileReferents | null>(null);
  // The pending env row. Two fields and no per-row edit mode: an existing variable is
  // changed by removing it and adding it again, which keeps every PUT a full replacement
  // (the daemon's contract) instead of a patch the client would have to compose.
  const [envKey, setEnvKey] = useState("");
  const [envValue, setEnvValue] = useState("");

  const load = useCallback(async (keep?: string | null) => {
    try {
      const { profiles: list } = await fetchSandboxProfiles();
      setProfiles(list);
      setSelected((cur) => {
        const want = keep ?? cur;
        if (want && list.some((p) => p.name === want)) return want;
        return list[0]?.name ?? null;
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load staging profiles");
    }
  }, []);

  // One fetch on mount — the panel is mounted only when the drill-down opens, so mounting
  // IS "the user opened the editor". `load` sets state after its `await`, which the rule
  // cannot see through; same trade-off and same disable as `FsExplorerModal`'s initial
  // navigate.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- async, see note above.
    void load();
  }, [load]);

  const current = profiles?.find((p) => p.name === selected) ?? null;

  /**
   * Write a profile's diff, then refresh both this panel and the parent's settings.
   *
   * Every `PUT` is a FULL replacement — `env` and `image` included — so each caller passes the
   * fields it is not changing verbatim. Threading that through one helper is what keeps "toggle
   * an entry" from quietly clearing the env or resetting the image source.
   */
  const write = useCallback(
    async (
      name: string,
      diff: {
        disabled: string[];
        extras: string[];
        env: Record<string, string>;
        image: SandboxProfileImage | null;
      },
    ) => {
      if (busy) return;
      setBusy(true);
      setError(null);
      try {
        await saveSandboxProfile(name, diff);
        await load(name);
        onChanged();
      } catch (e) {
        setError(e instanceof Error ? e.message : "Failed to save the profile");
      } finally {
        setBusy(false);
      }
    },
    [busy, load, onChanged],
  );

  /**
   * Toggle one entry. Only a BUILT-IN DEFAULT entry is toggleable: unchecking adds it to
   * `disabled`, re-checking removes it. An extra is removed by dropping it from `extras`,
   * never by "unchecking" it — conflating the two would make the stored diff ambiguous
   * (and the daemon rejects a `disabled` that is not a default entry).
   */
  const toggleDefault = (entry: SandboxProfileEntry) => {
    if (!current || !entry.from_default) return;
    const disabled = entry.enabled
      ? [...current.disabled, entry.path]
      : current.disabled.filter((d) => d !== entry.path);
    void write(current.name, {
      disabled,
      extras: current.extras,
      env: current.env,
      image: current.image,
    });
  };

  const removeExtra = (path: string) => {
    if (!current) return;
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras.filter((e) => e !== path),
      env: current.env,
      image: current.image,
    });
  };

  const addExtra = (abs: string) => {
    if (!current) return;
    const rel = relativiseToHome(abs, home);
    if (!rel) {
      setError(
        `\`${abs}\` must live under your home directory${home ? ` (${home})` : ""} — an entry is a path relative to \`$HOME\`.`,
      );
      return;
    }
    if (current.extras.includes(rel)) return;
    void write(current.name, {
      disabled: current.disabled,
      extras: [...current.extras, rel],
      env: current.env,
      image: current.image,
    });
  };

  /**
   * Add (or replace) one environment variable (#468).
   *
   * The two refusals handled inline are the ones the user can see before a round-trip: a
   * blank name, and a name PDO poses itself. `reserved_env_keys` comes from the daemon —
   * hard-coding the three here would drift the day a fourth run-constant appears. Every
   * other rule (the `[A-Za-z_][A-Za-z0-9_]*` grammar, multi-line values) is left to the
   * daemon's 400, which this panel already surfaces verbatim: duplicating a grammar in two
   * languages is exactly the drift #373 cost us.
   */
  const addEnv = () => {
    if (!current) return;
    const key = envKey.trim();
    if (!key) return;
    if (current.reserved_env_keys.includes(key)) {
      setError(
        `\`${key}\` is set by PDO for every sandboxed Run and cannot be overridden — the container's home, its daemon URL and its Run id all depend on it.`,
      );
      return;
    }
    setEnvKey("");
    setEnvValue("");
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras,
      env: { ...current.env, [key]: envValue },
      image: current.image,
    });
  };

  const removeEnv = (key: string) => {
    if (!current) return;
    // A full replacement, so "remove" is literally "PUT the map without that key".
    const env = Object.fromEntries(
      Object.entries(current.env).filter(([k]) => k !== key),
    );
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras,
      env,
      image: current.image,
    });
  };

  /**
   * Write the profile's image source (#467). `null` clears it — a FULL replacement, so that is
   * literally how "go back to PDO's default image" is expressed (#471).
   *
   * Nothing is validated here beyond emptiness: the absolute-path rule, the existence check and
   * the ref grammar are all the daemon's 400s, which this panel surfaces verbatim. Re-deriving any
   * of them in TypeScript is exactly the drift #373 cost us — and the existence check is not even
   * derivable in a browser.
   */
  const writeImage = (image: SandboxProfileImage | null) => {
    if (!current) return;
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras,
      env: current.env,
      image,
    });
  };

  const create = async () => {
    const name = newName.trim();
    if (!name || busy) return;
    setBusy(true);
    setError(null);
    try {
      // A blank diff: "materialise this profile exactly as the current default", which is
      // the starting point for unchecking something.
      await saveSandboxProfile(name, { disabled: [], extras: [], env: {}, image: null });
      setNewName("");
      setCreating(false);
      await load(name);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to create the profile");
    } finally {
      setBusy(false);
    }
  };

  /** Ask the daemon who points at the profile BEFORE opening the confirmation. */
  const askDelete = async (name: string) => {
    setError(null);
    try {
      setConfirmDelete(await fetchSandboxProfileReferents(name));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to look up referents");
    }
  };

  const doDelete = async (name: string) => {
    setBusy(true);
    setError(null);
    try {
      await deleteSandboxProfile(name);
      setConfirmDelete(null);
      await load(null);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to delete the profile");
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div
        className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-4"
        data-testid="staging-profiles-panel"
      >
        {/* ── the profile list ── */}
        <div className="flex flex-col gap-1">
          {profiles == null ? (
            <div className="text-fg-4" style={{ fontSize: "12px" }} data-testid="staging-profiles-loading">
              Loading…
            </div>
          ) : (
            profiles.map((p) => (
              <div
                key={p.name}
                className={`flex items-center gap-2 rounded-md border px-2.5 py-1.5 transition-colors ${
                  p.name === selected
                    ? "border-acc bg-bg-3"
                    : "border-line-strong bg-bg-3 hover:border-line"
                }`}
              >
                <button
                  type="button"
                  onClick={() => setSelected(p.name)}
                  data-testid={`staging-profile-row-${p.name}`}
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                >
                  <span className="truncate font-mono text-fg" style={{ fontSize: "12px" }}>
                    {p.name}
                  </span>
                  <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                    {p.materialised ? "edited" : p.virtual ? "built-in" : ""}
                  </span>
                </button>
                {/* Only a MATERIALISED row can be deleted; an unedited built-in default has
                    no row to delete (the daemon 404s), so the button is not offered. */}
                {p.materialised && (
                  <button
                    type="button"
                    onClick={() => void askDelete(p.name)}
                    aria-label={`Delete ${p.name}`}
                    data-testid={`staging-profile-delete-${p.name}`}
                    className="shrink-0 rounded p-1 text-fg-4 transition-colors hover:bg-bg-5 hover:text-st-failed"
                  >
                    <Trash2 size={13} />
                  </button>
                )}
              </div>
            ))
          )}

          {creating ? (
            <div className="mt-1 flex items-center gap-2">
              <input
                autoFocus
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void create();
                  if (e.key === "Escape") setCreating(false);
                }}
                placeholder="full-no-mcp"
                data-testid="staging-profile-new-name"
                className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                style={{ fontSize: "12px" }}
              />
              <button
                type="button"
                onClick={() => void create()}
                disabled={busy || newName.trim().length === 0}
                data-testid="staging-profile-create"
                className="shrink-0 rounded-md bg-acc px-2.5 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
                style={{ fontSize: "11px" }}
              >
                Create
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setCreating(true)}
              data-testid="staging-profile-new"
              className="mt-1 flex items-center gap-1.5 self-start rounded-md px-1 py-1 text-fg-3 transition-colors hover:text-fg"
              style={{ fontSize: "11px" }}
            >
              <Plus size={12} /> New profile
            </button>
          )}
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Lowercase letters, digits and <span className="font-mono">-</span>. A new profile
            starts as a copy of the built-in default; unchecking an entry stores the
            difference, so a later PDO release that adds an entry still shows up here.
          </div>
        </div>

        {error && (
          <div
            className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
            style={{ fontSize: "11.5px" }}
            data-testid="staging-profiles-error"
          >
            {error}
          </div>
        )}

        {/* ── the selected profile's entries ── */}
        {current && (
          <div className="flex flex-col gap-3 border-t border-line pt-4">
            <h3 className="font-medium text-fg-2" style={{ fontSize: "12px" }}>
              <span className="font-mono">{current.name}</span> — entries
            </h3>

            {/* The FLOOR, read-only. Without this block a `minimal` profile's screen looks
                broken and the user wrongly concludes the container starts with no
                credentials. These are guarantees, satisfied by a host copy OR by a
                fallback synthesis — never selectable, and refused as extras too. */}
            <div className="flex flex-col gap-1" data-testid="staging-profile-floor">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                Floor (always applied, not editable)
              </span>
              <ul className="flex flex-col gap-0.5">
                {current.floor.map((g) => (
                  <li key={g.id} className="flex items-baseline gap-2 text-fg-4" style={{ fontSize: "10.5px" }}>
                    <span>· {g.label}</span>
                    {g.path && <span className="truncate font-mono">{g.path}</span>}
                  </li>
                ))}
              </ul>
            </div>

            {/* From the default: checkable. `minimal` legitimately has none — say so
                explicitly, or the empty area reads as a loading failure. */}
            <div className="flex flex-col gap-1.5">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                From the default
              </span>
              {current.entries.filter((e) => e.from_default).length === 0 ? (
                <span
                  className="text-fg-4"
                  style={{ fontSize: "10.5px" }}
                  data-testid="staging-profile-no-default-entries"
                >
                  No entries — <span className="font-mono">{current.name}</span> <em>is</em> the
                  floor.
                </span>
              ) : (
                current.entries
                  .filter((e) => e.from_default)
                  .map((e) => (
                    <label
                      key={e.path}
                      className="flex cursor-pointer items-start gap-2"
                      data-testid={`staging-entry-${e.path}`}
                    >
                      <input
                        type="checkbox"
                        checked={e.enabled}
                        disabled={busy}
                        onChange={() => toggleDefault(e)}
                        className="mt-0.5 shrink-0 accent-acc"
                      />
                      <span className="flex min-w-0 flex-col">
                        <span className="flex items-baseline gap-2">
                          <span className="truncate font-mono text-fg" style={{ fontSize: "11.5px" }}>
                            {e.path}
                          </span>
                          <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                            {e.kind}
                          </span>
                          {e.exists === false && (
                            <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                              not on this host
                            </span>
                          )}
                        </span>
                        {e.note && (
                          <span className="text-fg-4" style={{ fontSize: "10px" }}>
                            {e.note}
                          </span>
                        )}
                      </span>
                    </label>
                  ))
              )}
            </div>

            {/* Extras: `$HOME` exceptions, added through the generic explorer. */}
            <div className="flex flex-col gap-1.5">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                Extras
              </span>
              {current.entries
                .filter((e) => !e.from_default)
                .map((e) => (
                  <div
                    key={e.path}
                    className="flex items-center gap-2"
                    data-testid={`staging-extra-${e.path}`}
                  >
                    <span className="truncate font-mono text-fg" style={{ fontSize: "11.5px" }}>
                      ~/{e.path}
                    </span>
                    <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                      {e.kind}
                    </span>
                    {e.sensitive && (
                      <span
                        className="shrink-0 text-st-await"
                        style={{ fontSize: "10px" }}
                        data-testid={`staging-extra-sensitive-${e.path}`}
                      >
                        secrets
                      </span>
                    )}
                    {e.exists === false && (
                      <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                        missing
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={() => removeExtra(e.path)}
                      disabled={busy}
                      aria-label={`Remove ${e.path}`}
                      data-testid={`staging-extra-remove-${e.path}`}
                      className="ml-auto shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-5 hover:text-fg-2 disabled:opacity-40"
                    >
                      <X size={12} />
                    </button>
                  </div>
                ))}
              <div className="flex gap-2 pt-0.5">
                {/* `FsExplorerModal` is consumed UNCHANGED (#431): two buttons, two mounts,
                    `mode="file"` and `mode="dir"`, both `showHidden` — the entries that
                    matter here are dotfiles. A third `mode` value or a `pickDirs` flag
                    would have to rewrite `handleRow`, the exact line whose frozen test
                    keeps `RepoCombobox` and the repo-explorer e2e independent. */}
                <button
                  type="button"
                  onClick={() => setPickerMode("file")}
                  data-testid="staging-extra-add-file"
                  className="rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc"
                  style={{ fontSize: "11px" }}
                >
                  <span className="inline-flex items-center gap-1.5">
                    <Search size={11} /> Add file…
                  </span>
                </button>
                <button
                  type="button"
                  onClick={() => setPickerMode("dir")}
                  data-testid="staging-extra-add-folder"
                  className="rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc"
                  style={{ fontSize: "11px" }}
                >
                  <span className="inline-flex items-center gap-1.5">
                    <Search size={11} /> Add folder…
                  </span>
                </button>
              </div>
              <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
                An extra is <strong>copied</strong> into the run's staging and mounted
                read-write in the container. The host file is never mutated — and never
                bind-mounted, so nothing the container writes can reach it.
              </div>
              {current.entries.some((e) => !e.from_default && e.sensitive) && (
                <div
                  className="text-st-await"
                  style={{ fontSize: "10.5px" }}
                  data-testid="staging-profile-sensitive-warning"
                >
                  This profile stages secrets ({current.sensitive_prefixes.join(", ")}). They
                  are copied, not shared — but the container can read them.
                </div>
              )}
            </div>

            {/* Environment (#468, ADR-0031 §8): posed as `-e KEY=value` at `docker create`,
                which is often the ONLY handle on a plugin-provided MCP server whose
                `.mcp.json` PDO does not control. */}
            <div className="flex flex-col gap-1.5" data-testid="staging-profile-env">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                Environment
              </span>
              {Object.entries(current.env).length === 0 ? (
                <span
                  className="text-fg-4"
                  style={{ fontSize: "10.5px" }}
                  data-testid="staging-profile-no-env"
                >
                  None — the container gets only the variables PDO sets itself.
                </span>
              ) : (
                Object.entries(current.env).map(([key, value]) => (
                  <div
                    key={key}
                    className="flex items-center gap-2"
                    data-testid={`staging-env-${key}`}
                  >
                    <span className="shrink-0 font-mono text-fg" style={{ fontSize: "11.5px" }}>
                      {key}
                    </span>
                    <span className="text-fg-4" style={{ fontSize: "11.5px" }}>
                      =
                    </span>
                    {/* Shown in clear, deliberately: the value is already in clear in the
                        database, in the Run's event file and in `docker inspect`. Masking it
                        would suggest PDO is protecting it. */}
                    <span
                      className="min-w-0 flex-1 truncate font-mono text-fg-2"
                      style={{ fontSize: "11.5px" }}
                      title={value}
                    >
                      {value === "" ? <em className="text-fg-4">(empty)</em> : value}
                    </span>
                    <button
                      type="button"
                      onClick={() => removeEnv(key)}
                      disabled={busy}
                      aria-label={`Remove ${key}`}
                      data-testid={`staging-env-remove-${key}`}
                      className="shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-5 hover:text-fg-2 disabled:opacity-40"
                    >
                      <X size={12} />
                    </button>
                  </div>
                ))
              )}
              <div className="flex items-center gap-2 pt-0.5">
                <input
                  value={envKey}
                  onChange={(e) => setEnvKey(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addEnv();
                  }}
                  placeholder="PUPPETEER_EXECUTABLE_PATH"
                  aria-label="Environment variable name"
                  data-testid="staging-env-new-key"
                  className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                  style={{ fontSize: "11px" }}
                />
                <input
                  value={envValue}
                  onChange={(e) => setEnvValue(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addEnv();
                  }}
                  placeholder="/usr/bin/chromium"
                  aria-label="Environment variable value"
                  data-testid="staging-env-new-value"
                  className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                  style={{ fontSize: "11px" }}
                />
                <button
                  type="button"
                  onClick={addEnv}
                  disabled={busy || envKey.trim().length === 0}
                  data-testid="staging-env-add"
                  className="shrink-0 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc disabled:opacity-40"
                  style={{ fontSize: "11px" }}
                >
                  Set
                </button>
              </div>
              <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
                Posed at container creation, for every session of the Run. Often the only way
                to configure an MCP server whose <span className="font-mono">.mcp.json</span>{" "}
                comes from a plugin. <span className="font-mono">{current.reserved_env_keys.join(", ")}</span>{" "}
                are set by PDO and refused here.
              </div>
              {/* Load-bearing copy, not a disclaimer: without it someone puts an API key
                  here believing it is a secret store. */}
              <div
                className="text-st-await"
                style={{ fontSize: "10.5px" }}
                data-testid="staging-profile-env-not-a-vault"
              >
                <strong>This is not a secret store.</strong> Values are stored in clear in the
                PDO database, copied into the Run's frozen event log, and readable with{" "}
                <span className="font-mono">docker inspect</span>. They are kept out of the
                daemon log (names only), and nothing else protects them.
              </div>
            </div>

            {/* Image (#467, ADR-0031 §9): WHICH container the Run gets, as opposed to what
                lands in its home. `key` on the profile name so switching profiles resets the
                draft — the reset is the mount, not an effect (the #385 lesson). */}
            <ProfileImageEditor
              key={current.name}
              profile={current}
              busy={busy}
              onWrite={writeImage}
            />

            {/* Signalled no-ops (ADR-0031 §2). Not errors: the default may LOSE an entry
                tomorrow, and unchecking one a future release will add must be remembered. */}
            {current.inactive_disabled.length > 0 && (
              <div
                className="text-fg-4"
                style={{ fontSize: "10.5px" }}
                data-testid="staging-profile-inactive-disabled"
              >
                Remembered but inactive (not in this version's default):{" "}
                <span className="font-mono">{current.inactive_disabled.join(", ")}</span>
              </div>
            )}
            {current.redundant_extras.length > 0 && (
              <div
                className="text-fg-4"
                style={{ fontSize: "10.5px" }}
                data-testid="staging-profile-redundant-extras"
              >
                Already in the default (kept in case it is removed later):{" "}
                <span className="font-mono">{current.redundant_extras.join(", ")}</span>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Footer says DONE, not Save: every edit above already went to the daemon. That
          difference is what makes the drill-down honest — nothing is batched behind it. */}
      {onDone && (
        <div className="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
          <button
            onClick={onDone}
            data-testid="staging-profiles-done"
            className="rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
            style={{ fontSize: "11.5px" }}
          >
            Done
          </button>
        </div>
      )}

      {pickerMode && (
        <FsExplorerModal
          mode={pickerMode}
          showHidden
          title={pickerMode === "file" ? "Add a file to stage" : "Add a folder to stage"}
          confirmLabel="Add to the profile"
          startPath={home ?? undefined}
          onPick={addExtra}
          onClose={() => setPickerMode(null)}
        />
      )}

      {confirmDelete && (
        <DeleteProfileDialog
          referents={confirmDelete}
          busy={busy}
          onCancel={() => setConfirmDelete(null)}
          onConfirm={() => void doDelete(confirmDelete.name)}
        />
      )}
    </>
  );
}

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
function ProfileImageEditor({
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

/**
 * Confirmation before deleting a profile — the "soft guard-rail" of ADR-0031 §7 (there is
 * no referential integrity in the database, and the `DELETE` is unconditional).
 *
 * It exists to say the two things nothing else in the UI says: deleting does **not**
 * repoint the referents, and their next Run **fails** rather than falling back to a
 * default — while live Runs already froze their entry list and are unaffected.
 *
 * `z-[60]`, the same layer as `FsExplorerModal`; they are never open at once.
 */
function DeleteProfileDialog({
  referents,
  busy,
  onCancel,
  onConfirm,
}: {
  referents: SandboxProfileReferents;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const pointing = referents.triggers.length + (referents.instance_default ? 1 : 0);
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(e) => {
        e.stopPropagation();
        onCancel();
      }}
    >
      <div
        className="w-[420px] rounded-lg border border-line bg-bg-4 p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="staging-profile-delete-dialog"
      >
        <h3 className="font-semibold text-fg" style={{ fontSize: "12.5px" }}>
          Delete profile <span className="font-mono">{referents.name}</span>?
        </h3>
        {pointing > 0 ? (
          <>
            <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px" }}>
              {pointing} thing{pointing === 1 ? "" : "s"} still point at it. Deleting will{" "}
              <strong>not</strong> repoint them — the next Run they produce{" "}
              <strong>fails</strong>, it does not fall back to a default.
            </p>
            <ul className="mt-1.5 flex flex-col gap-0.5" data-testid="staging-profile-referents">
              {referents.instance_default && (
                <li className="text-fg-3" style={{ fontSize: "10.5px" }}>
                  · Instance default sandbox
                </li>
              )}
              {referents.triggers.map((t) => (
                <li key={t.id} className="text-fg-3" style={{ fontSize: "10.5px" }}>
                  · Trigger <span className="font-mono">{t.name}</span>
                  {t.enabled ? "" : " (disabled)"}
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px" }}>
            Nothing points at it.
          </p>
        )}
        {referents.runs.length > 0 && (
          <p
            className="mt-2 text-fg-4"
            style={{ fontSize: "10.5px" }}
            data-testid="staging-profile-referent-runs"
          >
            {referents.runs.length} running Run{referents.runs.length === 1 ? "" : "s"} already
            froze their entry list at start and are unaffected.
          </p>
        )}
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            disabled={busy}
            data-testid="staging-profile-delete-confirm"
            className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
