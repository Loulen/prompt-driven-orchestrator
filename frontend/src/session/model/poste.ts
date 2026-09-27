/**
 * The **poste** of this browser (#867, ADR-0075 §4): a random id drawn on first
 * use and kept in `localStorage`, so every tab of the browser shares it and it
 * survives a reload. The terminal passes it when it opens its socket; the daemon
 * gives one pilot per terminal between postes, and none between two tabs of the
 * same poste.
 *
 * Guarded like the other `pdo.*` keys: with storage unavailable (private mode,
 * disabled) the id lives in memory for this page, which makes the page a poste
 * of its own — the right degradation, never a throw.
 */

const POSTE_KEY = "pdo.poste";
const ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const POSTE_LEN = 16;
/** What the daemon accepts; anything else would make the socket anonymous. */
const VALID = /^[A-Za-z0-9_-]{1,64}$/;

let inMemory: string | null = null;

function drawPoste(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(POSTE_LEN));
  let id = "";
  for (let i = 0; i < POSTE_LEN; i++) id += ALPHABET[bytes[i] % ALPHABET.length];
  return id;
}

export function posteId(): string {
  try {
    const stored = localStorage.getItem(POSTE_KEY);
    if (stored && VALID.test(stored)) return stored;
    const fresh = inMemory ?? drawPoste();
    localStorage.setItem(POSTE_KEY, fresh);
    inMemory = fresh;
    return fresh;
  } catch {
    inMemory ??= drawPoste();
    return inMemory;
  }
}

/** Test seam: forget the in-memory fallback. */
export function resetPosteForTests(): void {
  inMemory = null;
}
