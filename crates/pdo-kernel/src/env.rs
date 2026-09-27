//! The environment variables the instance configuration reads as its `env` tier
//! (ADR-0015: `stored → env → default`), and the per-daemon overrides read once
//! at boot.
//!
//! They live in the kernel, not in the module that consumes each value, so that
//! reading a setting never means importing the session manager or the sandbox.

/// The port a daemon — and the service unit — listens on by default.
pub const DEFAULT_PORT: u16 = 5172;

/// Where the CLI reaches the daemon when `PDO_DAEMON_URL` is unset.
pub const DEFAULT_DAEMON_URL: &str = "http://localhost:5172";

/// Replaces the `claude …` tail in the tmux script of a node session.
///
/// Read **once at daemon boot** (`DaemonConfig::from_env`) and then carried as
/// per-daemon config, never consulted in the spawn hot path. Tests seed the
/// override through the daemon config, not by mutating this process-global env
/// (which races across parallel test threads).
pub const TMUX_CMD_OVERRIDE_ENV: &str = "PDO_TMUX_CMD_OVERRIDE";

/// Points docker invocations at a fake executable (test/integration seam). Read
/// once at boot, like [`TMUX_CMD_OVERRIDE_ENV`], never in the core.
pub const DOCKER_CMD_OVERRIDE_ENV: &str = "PDO_DOCKER_CMD_OVERRIDE";

/// Supplies the instance-wide `default_model` (#347, ADR-0015). No baked-in
/// default: unset means the account default (no `--model`), byte-identical to
/// the legacy launch.
pub const DEFAULT_MODEL_ENV: &str = "PDO_DEFAULT_MODEL";

/// Supplies the instance-wide default harness (#550, ADR-0046). Unset or empty
/// falls through to the `claude` floor, byte-identical launch.
pub const DEFAULT_HARNESS_ENV: &str = "PDO_DEFAULT_HARNESS";

/// Overrides the reaper TTL (seconds). Default: 3600 (1 h).
pub const REAPER_TTL_SECS_ENV: &str = "PDO_REAPER_TTL_SECS";

/// Overrides the reaper sweep interval (seconds). Default: 60.
pub const REAPER_INTERVAL_SECS_ENV: &str = "PDO_REAPER_INTERVAL_SECS";

/// Overrides the library assistant's idle TTL (seconds). Default: 120 (#594,
/// ADR-0051).
pub const LIBASSIST_IDLE_TTL_SECS_ENV: &str = "PDO_LIBASSIST_IDLE_TTL_SECS";
