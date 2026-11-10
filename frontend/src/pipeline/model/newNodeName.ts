/**
 * The name a node gets when « + › Node » adds it: the type's base name, or
 * that name numbered from 2 when a card of the canvas already wears it.
 */
export function newNodeName(base: string, taken: Iterable<string>): string {
  const names = new Set(taken);
  if (!names.has(base)) return base;
  for (let n = 2; ; n++) {
    const name = `${base}-${n}`;
    if (!names.has(name)) return name;
  }
}
