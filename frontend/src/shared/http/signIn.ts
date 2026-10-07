/**
 * The sign-in gate of the UI (ADR-0081, 0.8): a daemon whose extension refuses
 * a request nobody identified answers `401` `{error: "unauthenticated",
 * redirect}`, `redirect` naming the extension's sign-in page. The UI goes
 * there once, telling it where to come back: `next=<path + query + hash>`, the
 * deep link included.
 *
 * Every call of {@link request} follows it; a WebSocket cannot read the status
 * of a refused upgrade, so its probe (`probedSocket.ts`, ADR-0083) asks first.
 */

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

let returnLink: string | null = null;

/**
 * The deep link of what the UI shows when its address has none: the app keeps
 * no Run in its URL (CORE-1), so the selected Run registers `#/runs/<id>` here
 * and a session lost on it comes back to it. `null` forgets it.
 */
export function setSignInReturnLink(link: string | null): void {
  returnLink = link;
}

/** Where the UI is: path, query and hash (the deep link, or the registered one). */
export function currentLocation(): string {
  const { pathname, search, hash } = window.location;
  return `${pathname}${search}${hash || (returnLink ?? "")}`;
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

/** Tests: forget a redirection. */
export function resetSignInForTests(): void {
  redirecting = false;
  returnLink = null;
}
