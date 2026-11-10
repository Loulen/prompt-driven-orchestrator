/**
 * The names of the agent profiles this browser has listed, kept after a
 * profile is deleted. A node names its profile by id only (ADR-0057: a live
 * reference, never a copy), so once the profile is gone the id is all the
 * pipeline holds; the warnings then say the name the user knew, where this
 * browser saw it, and the id otherwise.
 *
 * Kept in `localStorage`, read and written under try/catch. What this session
 * listed wins over what is stored (another tab may have stored older names),
 * and lasts for the session when storage is disabled or full.
 */
const KEY = "pdo.agentProfiles.names";

/** What this session listed, newest name per id. */
let session: Record<string, string> = {};

function stored(): Record<string, string> {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return Object.fromEntries(
        Object.entries(parsed as Record<string, unknown>).filter(
          (entry): entry is [string, string] => typeof entry[1] === "string",
        ),
      );
    }
  } catch {
    // corrupt or unreadable: nothing stored
  }
  return {};
}

/** Remember the names of `profiles`, as the daemon listed them. */
export function rememberProfileNames(profiles: readonly { id: string; name: string }[]): void {
  if (!Array.isArray(profiles)) return;
  if (profiles.every(({ id, name }) => session[id] === name)) return;
  session = { ...session, ...Object.fromEntries(profiles.map(({ id, name }) => [id, name])) };
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...stored(), ...session }));
  } catch {
    // quota or disabled: the names last for the session
  }
}

/** The name of profile `id` as last seen here, or its id. */
export function profileLabel(id: string): string {
  return session[id] ?? stored()[id] ?? id;
}

/** Forget what this session listed (tests: a fresh browser). */
export function forgetSessionProfileNames(): void {
  session = {};
}
