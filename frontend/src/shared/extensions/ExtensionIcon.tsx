import { useEffect, useState } from "react";

/** Whether each icon URL loaded: fetched once per page, shared by every button. */
const loadedIcons = new Map<string, boolean>();

function useIconLoaded(url: string | null | undefined): boolean {
  const [, settled] = useState(0);
  useEffect(() => {
    if (!url || loadedIcons.has(url)) return;
    let alive = true;
    const settle = (loaded: boolean) => {
      loadedIcons.set(url, loaded);
      if (alive) settled((n) => n + 1);
    };
    const probe = new Image();
    probe.onload = () => settle(true);
    probe.onerror = () => settle(false);
    probe.src = url;
    return () => {
      alive = false;
    };
  }, [url]);
  return Boolean(url) && loadedIcons.get(url as string) === true;
}

/**
 * An extension's icon: its SVG, drawn as a mask in the colour of the text
 * around it (hover and accent states apply as to a lucide icon); until it
 * loads, or without one, the label's first letter, boxed.
 */
export default function ExtensionIcon({
  url,
  label,
  size = 15,
}: {
  url?: string | null;
  label: string;
  size?: number;
}) {
  const loaded = useIconLoaded(url);
  if (url && loaded) {
    const mask = `url("${url}") center / contain no-repeat`;
    return (
      <span
        aria-hidden
        data-testid="extension-icon"
        className="inline-block shrink-0 bg-current"
        style={{ width: size, height: size, mask, WebkitMask: mask }}
      />
    );
  }
  return (
    <span
      aria-hidden
      data-testid="extension-icon-fallback"
      className="grid shrink-0 place-items-center rounded-[3px] border border-current font-semibold leading-none"
      style={{ width: size, height: size, fontSize: Math.round(size * 0.6) }}
    >
      {(label.trim()[0] ?? "?").toUpperCase()}
    </span>
  );
}
