/**
 * The one clipboard write of the interface: every Copy button of the core goes
 * through it, the terminal too, and `@pdo/extension-ui` hands it to the
 * extensions (since 0.11), so a copy works on an instance served over plain
 * http, where `navigator.clipboard` does not exist.
 */

/**
 * Put `text` on the system clipboard from a user gesture. Prefers the async
 * Clipboard API (secure contexts only: https or localhost) and falls back to
 * `execCommand("copy")` on a throwaway textarea, which is what still works on
 * `http://<vps>:5172`. Resolves to whether anything reported success; never
 * rejects.
 */
export async function copyToClipboard(text: string, doc: Document = document): Promise<boolean> {
  const nav = doc.defaultView?.navigator;
  if (nav?.clipboard?.writeText) {
    try {
      await nav.clipboard.writeText(text);
      return true;
    } catch {
      // Fall through: denied permission or insecure context.
    }
  }
  const ta = doc.createElement("textarea");
  ta.value = text;
  ta.setAttribute("readonly", "");
  ta.style.position = "fixed";
  ta.style.top = "0";
  ta.style.left = "0";
  ta.style.opacity = "0";
  ta.style.pointerEvents = "none";
  doc.body.appendChild(ta);
  const active = doc.activeElement as HTMLElement | null;
  try {
    ta.focus();
    ta.select();
    return typeof doc.execCommand === "function" && doc.execCommand("copy");
  } catch {
    return false;
  } finally {
    doc.body.removeChild(ta);
    active?.focus?.();
  }
}
