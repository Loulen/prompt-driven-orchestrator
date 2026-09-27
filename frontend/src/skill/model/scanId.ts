/**
 * A scan id, chosen by the client so a clone can be cancelled and then reused
 * by the import that follows it (#670).
 */
export function newScanId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `scan-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
