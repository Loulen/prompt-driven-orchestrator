# Maestro Runtime Preamble

You are node `mWllKR47` in pipeline `simple-bugfix`, iteration 1.

## Inputs

- `in`: read `/home/llenoir/Documents/perso/Maestro/.maestro/runs/20260512-152526-cc6b674/worktree/.maestro/artifacts/HmuRVQBx/iter-1/done.md`

## Outputs

- `out`: write to `/home/llenoir/Documents/perso/Maestro/.maestro/runs/20260512-152526-cc6b674/worktree/.maestro/artifacts/mWllKR47/iter-1/out.md`

## Source code edits

Your working directory `/home/llenoir/Documents/perso/Maestro/.maestro/runs/20260512-152526-cc6b674/nodes/mWllKR47/iter-1` is a **dedicated git worktree** of the project, on its own branch. Make **all** source code edits there — do not `cd` elsewhere to edit files. Read with relative paths or paths under this directory.

The input/output artefact paths above live in the *pipeline worktree* (a different directory, shared with other nodes). Treat those paths as read-only/write-only for artefacts; never edit source code there.

When you run `maestro complete`, your committed changes are automatically merged from this sub-worktree back into the pipeline worktree. Edits made outside this directory will be silently dropped from the merge.

## Completion

When you are done, signal completion by running:
```
maestro complete
```

If you cannot complete the task, signal failure:
```
maestro fail --reason "<description of the problem>"
```

---

If everything went according to plan, commit and merge to main locally.

Then Rebuild the app and tell the user how to restart it
