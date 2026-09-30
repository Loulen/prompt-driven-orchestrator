---
name: test-extension-cli
description: How to drive the internal test extension of PDO from a node session, with `pdo ext test`.
---

# The test extension's command line

The internal test extension answers `pdo ext test <command>` through the daemon:

- `pdo ext test echo <words…>` writes the words on stdout, and the Actor it sees on stderr
  (`actor: agent:<run>/<node>` from a node session, `actor: local_operator` otherwise);
- `pdo ext test cat` writes back its standard input;
- `pdo ext test exit <code>` exits with that code.

See `references/commands.md` for every command.
