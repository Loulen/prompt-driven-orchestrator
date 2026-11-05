// js-yaml ships no types and is only a test-time dependency (the shared
// path-coherence fixture corpus is YAML, read by the TS mirror's test).
declare module "js-yaml" {
  export function load(source: string): unknown;
}
