import { useEffect, useRef } from "react";

/** The first image of a paste or a drop, else `null`. */
export function imageOf(data: DataTransfer | null): File | null {
  if (!data) return null;
  for (const file of Array.from(data.files ?? [])) {
    if (file.type.startsWith("image/")) return file;
  }
  for (const item of Array.from(data.items ?? [])) {
    if (item.kind === "file" && item.type.startsWith("image/")) return item.getAsFile();
  }
  return null;
}

/**
 * Takes an image pasted anywhere in the page while the window is open (Ctrl+V):
 * a paste of text goes on to its field.
 */
export function usePastedImage(onImage: (image: File) => void) {
  const take = useRef(onImage);
  useEffect(() => {
    take.current = onImage;
  });
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const image = imageOf(e.clipboardData);
      if (!image) return;
      e.preventDefault();
      take.current(image);
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, []);
}
