Your input is the incident diagnosis: the cause, the evidence and the suggested fix.
Fix it through PDO: launch one `implement-review` run with a task that states the cause and the expected fix, then wait for it with `pdo run wait`.
In `report`, set `fixed` to `true` once the child run has passed its review, with the link to its branch. Otherwise set it to `false` and say what is still broken.
