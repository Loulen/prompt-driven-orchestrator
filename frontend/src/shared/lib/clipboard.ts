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

/** `image` as a PNG: the one image type every clipboard takes. */
async function asPng(image: Blob): Promise<Blob> {
  if (image.type === "image/png") return image;
  const bitmap = await createImageBitmap(image);
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext("2d")?.drawImage(bitmap, 0, 0);
  return new Promise((resolve, reject) =>
    canvas.toBlob((png) => (png ? resolve(png) : reject(new Error("no PNG"))), "image/png"),
  );
}

/**
 * Put `image` on the system clipboard, as a PNG, from a user gesture (a
 * screenshot to paste elsewhere). Only the async Clipboard API carries an
 * image: on a page served over plain http, it does not exist. Resolves to
 * whether it worked; never rejects.
 */
export async function copyImageToClipboard(image: Blob, doc: Document = document): Promise<boolean> {
  const nav = doc.defaultView?.navigator;
  const Item = doc.defaultView?.ClipboardItem;
  if (!nav?.clipboard?.write || !Item) return false;
  try {
    // The PNG as a promise: the write starts within the gesture.
    await nav.clipboard.write([new Item({ "image/png": asPng(image) })]);
    return true;
  } catch {
    return false;
  }
}
