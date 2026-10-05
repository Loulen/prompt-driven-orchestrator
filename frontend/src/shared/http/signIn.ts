/**
 * The sign-in gate of the UI (ADR-0081, 0.8): a daemon whose extension refuses
 * a request nobody identified answers `401` `{error: "unauthenticated",
 * redirect}`, `redirect` naming the extension's sign-in page. The UI goes
 * there once, telling it where to come back: `next=<path + query + hash>`, the
 * deep link included.
 *
 * Every call of {@link request} follows it; `/ws` and a terminal's PTY cannot
 * read the status of a refused upgrade, so their sockets ask {@link probeSignIn}
 * when they close.
 */

/** The close code of a socket whose upgrade failed (a refused one included). */
export const ABNORMAL_CLOSURE = 1006;

/** The `error` slug of a request the extensions did not admit. */
export const UNAUTHENTICATED = "unauthenticated";

/** The sign-in page a refusal body names, if it names one. */
export function signInRedirect(body: unknown): string | null {
  const b = body as { error?: unknown; redirect?: unknown } | null | undefined;
  return b?.error === UNAUTHENTICATED && typeof b.redirect === "string" && b.redirect !== ""
    ? b.redirect
    : null;
}

/** `redirect`, with `next=<here>` appended to its query. */
export function signInUrl(redirect: string, here: string): string {
  const [base, hash = ""] = redirect.split("#", 2);
  const separator = base.includes("?") ? "&" : "?";
  return `${base}${separator}next=${encodeURIComponent(here)}${hash ? `#${hash}` : ""}`;
}

/** Where the UI is: path, query and hash (the deep link). */
export function currentLocation(): string {
  const { pathname, search, hash } = window.location;
  return `${pathname}${search}${hash}`;
}

let redirecting = false;

/** Whether the UI is on its way to the sign-in page. */
export function isRedirectingToSignIn(): boolean {
  return redirecting;
}

/** Go to the sign-in page `redirect`, once, coming back here after. */
export function followSignIn(redirect: string): void {
  if (redirecting) return;
  redirecting = true;
  window.location.assign(signInUrl(redirect, currentLocation()));
}

/** Follow the sign-in page `body` names, if any. Whether it did. */
export function followSignInOf(body: unknown): boolean {
  const redirect = signInRedirect(body);
  if (redirect === null) return false;
  followSignIn(redirect);
  return true;
}

/**
 * Ask the daemon whether the UI is still admitted (`GET /actors/me`), and go
 * to the sign-in page if it is not. What a socket does when it closes: a
 * refused upgrade tells the browser nothing.
 */
export async function probeSignIn(): Promise<void> {
  if (redirecting) return;
  try {
    const resp = await fetch("/actors/me");
    if (resp.status !== 401) return;
    followSignInOf(await resp.json().catch(() => null));
  } catch {
    // The daemon is down: the socket's own reconnection says so.
  }
}

/** Tests: forget a redirection. */
export function resetSignInForTests(): void {
  redirecting = false;
}
