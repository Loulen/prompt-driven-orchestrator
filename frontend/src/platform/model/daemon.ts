/**
 * Hand-written shape of the daemon-level payload that has no Rust DTO yet
 * (built with `json!` on the daemon side): the session counter. The version
 * check's live in `./update`, the `/ws` frames in `run/model/liveStream`.
 */

import type { ServiceHealth } from "../../shared/generated/ServiceHealth";

/**
 * Live NodeRun-session count, the configured global cap, the daemon version,
 * and the persistent-service health, for the bottom status bar (#159 /
 * ADR-0012, #139, #156). Manager sessions are excluded. `version` and
 * `service` are absent until the daemon has responded.
 */
export interface DaemonStatus {
  live: number;
  cap: number;
  version?: string;
  service?: ServiceHealth;
}
