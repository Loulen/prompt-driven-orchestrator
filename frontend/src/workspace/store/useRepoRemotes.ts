import { useEffect, useState } from "react";
import { fetchRepoRemote } from "../api/repos";

/**
 * The remote of each of `paths` (CORE-96: shown under a repository of a
 * Projet). Read once per path while the page is open; `undefined` until it
 * answered, `null` for a repository without a remote or one git cannot read.
 */
export function useRepoRemotes(paths: string[]): Map<string, string | null> {
  const [remotes, setRemotes] = useState<Map<string, string | null>>(() => new Map());
  const key = paths.join("\n");
  useEffect(() => {
    let current = true;
    const missing = key ? key.split("\n").filter((path) => !remotes.has(path)) : [];
    for (const path of missing) {
      fetchRepoRemote(path)
        .then((answer) => answer.remote)
        .catch(() => null)
        .then((remote) => {
          if (current) setRemotes((known) => new Map(known).set(path, remote));
        });
    }
    return () => {
      current = false;
    };
    // `remotes` is read, not followed: a path is asked once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return remotes;
}
