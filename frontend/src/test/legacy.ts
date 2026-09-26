import type { PipelineScope } from "../pipeline/model/pipeline";

/**
 * A `/pipelines` scope an older daemon sent (`repo`, `library`…). The current
 * daemon only ever sends `instance`, which is what the generated type says;
 * the tests that pin the client's tolerance of an older response go through
 * this cast.
 */
export function legacyScope(scope: PipelineScope): "instance" {
  return scope as "instance";
}
