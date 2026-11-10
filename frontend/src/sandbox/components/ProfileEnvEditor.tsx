import { useState } from "react";
import { ShieldAlert, X } from "lucide-react";
import { Button, IconButton, TextInput } from "../../shared/ui/kit";
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
        `\`${key}\` is set by the daemon for every sandboxed Run and cannot be overridden — the container's home, its daemon URL and its Run id all depend on it.`,
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
    <div className="set-group" data-testid="staging-profile-env">
      <span
        className="d-caps set-group__head"
        title={`Posed at container creation, for every session of the Run; ${profile.reserved_env_keys.join(", ")} are set by the daemon and refused`}
      >
        Environment
      </span>
      {Object.entries(profile.env).length === 0 ? (
        <span className="set-faint text-sm" data-testid="staging-profile-no-env">
          None
        </span>
      ) : (
        Object.entries(profile.env).map(([key, value]) => (
          <div key={key} className="set-rrow" data-testid={`staging-env-${key}`}>
            <span className="set-mono">{key}</span>
            <span className="set-faint">=</span>
            {/* Shown in clear, deliberately: the value is already in clear in the
                database, in the Run's event file and in `docker inspect`. Masking it
                would suggest it is protected. */}
            <span className="set-mono set-ellipsis set-rrow__path text-text-2" title={value}>
              {value === "" ? <em className="set-faint">(empty)</em> : value}
            </span>
            <IconButton
              icon={X}
              label={`Remove ${key}`}
              size="sm"
              onClick={() => removeEnv(key)}
              disabled={busy}
              data-testid={`staging-env-remove-${key}`}
            />
          </div>
        ))
      )}
      <div className="set-line pt-1">
        <TextInput
          mono
          value={envKey}
          onChange={(e) => setEnvKey(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addEnv();
          }}
          placeholder="PUPPETEER_EXECUTABLE_PATH"
          aria-label="Environment variable name"
          data-testid="staging-env-new-key"
        />
        <TextInput
          mono
          value={envValue}
          onChange={(e) => setEnvValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addEnv();
          }}
          placeholder="/usr/bin/chromium"
          aria-label="Environment variable value"
          data-testid="staging-env-new-value"
        />
        <Button size="sm" onClick={addEnv} disabled={busy || envKey.trim().length === 0} data-testid="staging-env-add">
          Set
        </Button>
      </div>
      {/* Load-bearing, not a disclaimer: without it someone puts an API key here
          believing it is a secret store. */}
      <div
        className="set-warn"
        title="Stored in clear in the database, copied into the Run's frozen event log, readable with docker inspect; kept out of the daemon log (names only)"
        data-testid="staging-profile-env-not-a-vault"
      >
        <ShieldAlert size={13} strokeWidth={1.5} aria-hidden />
        <span>Not a secret store</span>
        <span className="set-faint set-ellipsis">clear in the database · docker inspect</span>
      </div>
    </div>
  );
}
