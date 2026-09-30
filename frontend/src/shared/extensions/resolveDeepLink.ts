import { elementsAt } from "./catalogue";
import type { DeepLink, OpenPage } from "./deepLink";
import type { LoadedExtension } from "./store";

/**
 * The page `link` opens, or why it cannot: the extension is absent, its
 * interface is not loaded, or it has no page at that location. A deep link
 * opens the extension's first page there, whatever its `when` (an address does
 * not depend on the selection).
 */
export function resolveDeepLink(
  link: DeepLink,
  extensions: LoadedExtension[],
): { page: OpenPage } | { error: string } {
  const extension = extensions.find((e) => e.entry.id === link.extension);
  if (!extension) {
    return { error: `Extension “${link.extension}” is not installed: this link can't be opened.` };
  }
  const name = extension.entry.name;
  if (extension.status !== "ready") {
    return { error: `The interface of the extension ${name} is not loaded: this link can't be opened.` };
  }
  const page =
    link.location &&
    elementsAt(extensions, link.location).find((e) => e.extension.id === link.extension);
  if (!link.location || !page) {
    return { error: `The extension ${name} has no page for this link: it can't be opened.` };
  }
  return {
    page: { key: page.key, extension: link.extension, location: link.location, path: link.path },
  };
}

