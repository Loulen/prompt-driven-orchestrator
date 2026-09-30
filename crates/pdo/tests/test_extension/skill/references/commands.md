# Commands of `pdo ext test`

| Command | stdout | stderr | Exit code |
|---|---|---|---|
| `echo <words…>` | the words, space-separated | `actor: <actor>` | 0 |
| `cat` | the standard input | `actor: <actor>` | 0 |
| `exit <code>` | nothing | `actor: <actor>` | `<code>` |
| `panic` | nothing | nothing | 1 (the extension failed) |
| anything else | nothing | `unknown command` | 2 |
