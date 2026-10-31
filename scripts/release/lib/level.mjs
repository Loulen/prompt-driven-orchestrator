// The semver level of a release, from the conventional commits since the last
// tag (ADR draft-version-changelog-et-numero-d-adr-se-fixent-a-la-fusion-vers-main).
// A commit is `{ message, files }`: its full message and the paths it touches.

const LEVELS = ["patch", "minor", "major"];

// The paths of the extensions' contract: the Rust API, the shared module of the
// front and the stylesheet and script it serves under /shared/.
const API_PATHS = ["crates/pdo-extension-api/", "frontend/src/shared/extensions/", "frontend/public/shared/"];

const SUBJECT = /^(\w+)(\([^)]*\))?(!)?:/;

/** The level one commit asks for, or null for a merge commit. */
function commitLevel(message) {
  const subject = message.split("\n", 1)[0];
  if (subject.startsWith("Merge ")) return null;
  const match = SUBJECT.exec(subject);
  if (match?.[3] || /^BREAKING[ -]CHANGE:/m.test(message)) return "major";
  return match?.[1] === "feat" ? "minor" : "patch";
}

/**
 * The level the commits propose: a breaking change (`!` or a `BREAKING CHANGE:`
 * footer) is a major, a `feat` a minor, any other commit a patch, since a push
 * to `main` is a release. Merge commits weigh nothing; no commit, no level.
 * `reasons` are the subjects that decided it.
 */
export function proposeLevel(commits) {
  let level = null;
  let reasons = [];
  for (const { message } of commits) {
    const own = commitLevel(message);
    if (own === null) continue;
    const subject = message.split("\n", 1)[0];
    if (level === null || LEVELS.indexOf(own) > LEVELS.indexOf(level)) {
      level = own;
      reasons = [subject];
    } else if (own === level && own !== "patch") {
      reasons.push(subject);
    }
  }
  return { level, reasons };
}

/**
 * The level of the extension API: that of the commits touching its paths, a
 * major counting as a minor while the API is 0.x (cargo reads a 0.x minor as a
 * break). Untouched, it moves by a patch, which every `^0.N` requirement of an
 * extension still accepts.
 */
export function apiLevel(commits, apiVersion) {
  const touching = commits.filter(({ files }) => files.some((f) => API_PATHS.some((p) => f.startsWith(p))));
  const proposed = proposeLevel(touching);
  if (proposed.level === null) return { level: "patch", reasons: [] };
  if (proposed.level === "major" && apiVersion.startsWith("0.")) return { ...proposed, level: "minor" };
  return proposed;
}

/** `version` raised by `level`, the lower parts reset. */
export function bump(version, level) {
  const parts = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
  if (!parts) throw new Error(`${version} is not a version (X.Y.Z)`);
  const [major, minor, patch] = parts.slice(1).map(Number);
  switch (level) {
    case "major":
      return `${major + 1}.0.0`;
    case "minor":
      return `${major}.${minor + 1}.0`;
    case "patch":
      return `${major}.${minor}.${patch + 1}`;
    default:
      throw new Error(`unknown level ${level}: patch, minor or major`);
  }
}

export { LEVELS };
