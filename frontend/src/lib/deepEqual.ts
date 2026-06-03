// Structural deep equality for JSON-like values.
//
// Object key *order* is deliberately ignored: several pipeline fields
// (variables, port frontmatter, switch when-clauses) cross the wire as maps
// whose serialization order is nondeterministic on the daemon side (Rust
// HashMap). Two payloads that differ only in key order are the same value.
// Array order is significant — nodes/edges are ordered lists.
export function deepEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) {
      return false;
    }
    return a.every((v, i) => deepEqual(v, b[i]));
  }
  if (
    typeof a !== "object" ||
    typeof b !== "object" ||
    a === null ||
    b === null
  ) {
    return false;
  }
  const recA = a as Record<string, unknown>;
  const recB = b as Record<string, unknown>;
  const keysA = Object.keys(recA);
  const keysB = Object.keys(recB);
  if (keysA.length !== keysB.length) return false;
  return keysA.every(
    (k) =>
      Object.prototype.hasOwnProperty.call(recB, k) &&
      deepEqual(recA[k], recB[k]),
  );
}
