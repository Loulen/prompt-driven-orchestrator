# Contributing to PDO

## License

PDO is [MIT licensed](LICENSE). By opening a pull request you agree that your contribution
is released under the same license. Only contribute code you have the right to
contribute: work done under an employment or client contract may belong to that employer
or client. Agent-generated code is fine, and the project itself is built that way; you
remain the author of record.

The maintainer does not intend to change the license of this code. If that ever changed,
it would be announced in the CHANGELOG with a major version bump, and previous versions
would remain MIT.

## Workflow

The git flow lives in `.agents/skills/git-flow/SKILL.md`. In short: branch from `main`
(`feature/<issue>-<slug>` or `fix/<issue>-<slug>`), open a PR against `main`, and let CI
run. A maintainer reviews and merges. Nobody merges their own PR.

Before pushing:

```bash
make check        # cargo check, frontend typecheck, README support table
make test         # daemon and frontend tests
```

Commit messages follow Conventional Commits with a French or English body, as the rest of
the history does. Reference the issue in the title.

## Reporting a security issue

Do not open a public issue. Email the maintainer at the address on the GitHub profile.
