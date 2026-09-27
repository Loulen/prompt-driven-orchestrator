import { useState } from "react";
import { X } from "lucide-react";
import type { SandboxProfile } from "../model/sandbox";

/**
 * The profile's environment (#468, ADR-0031 §8): posed as `-e KEY=value` at `docker
 * create`, which is often the ONLY handle on a plugin-provided MCP server whose
 * `.mcp.json` PDO does not control.
 */
export default function ProfileEnvEditor({
  profile,
  busy,
  onWrite,
  onRefuse,
}: {
  profile: SandboxProfile;
  busy: boolean;
  /** Write the whole env map (a FULL replacement). */
  onWrite: (env: Record<string, string>) => void;
  /** Say why a variable was refused without a round-trip. */
  onRefuse: (message: string) => void;
}) {
  // The pending env row. Two fields and no per-row edit mode: an existing variable is
  // changed by removing it and adding it again, which keeps every PUT a full replacement
  // (the daemon's contract) instead of a patch the client would have to compose.
  const [envKey, setEnvKey] = useState("");
  const [envValue, setEnvValue] = useState("");

  /**
   * Add (or replace) one environment variable (#468).
   *
   * The two refusals handled inline are the ones the user can see before a round-trip: a
   * blank name, and a name PDO poses itself. `reserved_env_keys` comes from the daemon —
   * hard-coding the three here would drift the day a fourth run-constant appears. Every
   * other rule (the `[A-Za-z_][A-Za-z0-9_]*` grammar, multi-line values) is left to the
   * daemon's 400, which this panel already surfaces verbatim: duplicating a grammar in two
   * languages is exactly the drift #373 cost us.
   */
  const addEnv = () => {
    const key = envKey.trim();
    if (!key) return;
    if (profile.reserved_env_keys.includes(key)) {
      onRefuse(
        `\`${key}\` is set by PDO for every sandboxed Run and cannot be overridden — the container's home, its daemon URL and its Run id all depend on it.`,
      );
      return;
    }
    setEnvKey("");
    setEnvValue("");
    onWrite({ ...profile.env, [key]: envValue });
  };

  const removeEnv = (key: string) => {
    // A full replacement, so "remove" is literally "PUT the map without that key".
    onWrite(Object.fromEntries(Object.entries(profile.env).filter(([k]) => k !== key)));
  };

  return (
    <div className="flex flex-col gap-1.5" data-testid="staging-profile-env">
      <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
        Environment
      </span>
      {Object.entries(profile.env).length === 0 ? (
        <span
          className="text-fg-4"
          style={{ fontSize: "10.5px" }}
          data-testid="staging-profile-no-env"
        >
          None — the container gets only the variables PDO sets itself.
        </span>
      ) : (
        Object.entries(profile.env).map(([key, value]) => (
          <div
            key={key}
            className="flex items-center gap-2"
            data-testid={`staging-env-${key}`}
          >
            <span className="shrink-0 font-mono text-fg" style={{ fontSize: "11.5px" }}>
              {key}
            </span>
            <span className="text-fg-4" style={{ fontSize: "11.5px" }}>
              =
            </span>
            {/* Shown in clear, deliberately: the value is already in clear in the
                database, in the Run's event file and in `docker inspect`. Masking it
                would suggest PDO is protecting it. */}
            <span
              className="min-w-0 flex-1 truncate font-mono text-fg-2"
              style={{ fontSize: "11.5px" }}
              title={value}
            >
              {value === "" ? <em className="text-fg-4">(empty)</em> : value}
            </span>
            <button
              type="button"
              onClick={() => removeEnv(key)}
              disabled={busy}
              aria-label={`Remove ${key}`}
              data-testid={`staging-env-remove-${key}`}
              className="shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-5 hover:text-fg-2 disabled:opacity-40"
            >
              <X size={12} />
            </button>
          </div>
        ))
      )}
      <div className="flex items-center gap-2 pt-0.5">
        <input
          value={envKey}
          onChange={(e) => setEnvKey(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addEnv();
          }}
          placeholder="PUPPETEER_EXECUTABLE_PATH"
          aria-label="Environment variable name"
          data-testid="staging-env-new-key"
          className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
          style={{ fontSize: "11px" }}
        />
        <input
          value={envValue}
          onChange={(e) => setEnvValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addEnv();
          }}
          placeholder="/usr/bin/chromium"
          aria-label="Environment variable value"
          data-testid="staging-env-new-value"
          className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
          style={{ fontSize: "11px" }}
        />
        <button
          type="button"
          onClick={addEnv}
          disabled={busy || envKey.trim().length === 0}
          data-testid="staging-env-add"
          className="shrink-0 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc disabled:opacity-40"
          style={{ fontSize: "11px" }}
        >
          Set
        </button>
      </div>
      <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
        Posed at container creation, for every session of the Run. Often the only way
        to configure an MCP server whose <span className="font-mono">.mcp.json</span>{" "}
        comes from a plugin. <span className="font-mono">{profile.reserved_env_keys.join(", ")}</span>{" "}
        are set by PDO and refused here.
      </div>
      {/* Load-bearing copy, not a disclaimer: without it someone puts an API key
          here believing it is a secret store. */}
      <div
        className="text-st-await"
        style={{ fontSize: "10.5px" }}
        data-testid="staging-profile-env-not-a-vault"
      >
        <strong>This is not a secret store.</strong> Values are stored in clear in the
        PDO database, copied into the Run's frozen event log, and readable with{" "}
        <span className="font-mono">docker inspect</span>. They are kept out of the
        daemon log (names only), and nothing else protects them.
      </div>
    </div>
  );
}
