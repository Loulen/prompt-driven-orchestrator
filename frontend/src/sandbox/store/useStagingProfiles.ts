import { useCallback, useEffect, useState } from "react";
import {
  deleteSandboxProfile,
  fetchSandboxProfileReferents,
  fetchSandboxProfiles,
  saveSandboxProfile,
} from "../api/profiles";
import type {
  SandboxProfile,
  SandboxProfileDiff,
  SandboxProfileReferents,
} from "../model/sandbox";

/**
 * The staging profiles as the editor holds them: the list, the selected one, and the
 * writes. Every write is its own `PUT` (profiles are their own REST resource, never
 * batched behind the grouped `PUT /settings`), followed by a refresh of the list and a
 * call to `onChanged` so the parent can refetch `GET /settings`.
 */
export function useStagingProfiles(onChanged: () => void) {
  const [profiles, setProfiles] = useState<SandboxProfile[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<SandboxProfileReferents | null>(null);

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

  // One fetch on mount — the panel is mounted with its Settings section (#691), so mounting
  // IS "the user opened the Sandbox & worktrees page". `load` sets state after its `await`, which the rule
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
    async (name: string, diff: SandboxProfileDiff) => {
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

  /** Write `current` with `change` applied over its four fields. */
  const writeCurrent = (change: Partial<SandboxProfileDiff>) => {
    if (!current) return;
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras,
      env: current.env,
      image: current.image,
      ...change,
    });
  };

  /** Create `name` from a blank diff; `true` once it exists. */
  const create = async (name: string): Promise<boolean> => {
    if (!name || busy) return false;
    setBusy(true);
    setError(null);
    try {
      // A blank diff: "materialise this profile exactly as the current default", which is
      // the starting point for unchecking something.
      await saveSandboxProfile(name, { disabled: [], extras: [], env: {}, image: null });
      await load(name);
      onChanged();
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to create the profile");
      return false;
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

  return {
    profiles,
    selected,
    setSelected,
    current,
    error,
    setError,
    busy,
    writeCurrent,
    create,
    confirmDelete,
    askDelete,
    cancelDelete: () => setConfirmDelete(null),
    doDelete,
  };
}
