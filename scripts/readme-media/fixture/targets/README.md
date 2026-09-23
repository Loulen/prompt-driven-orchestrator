# Target pipelines

Drawn by the maintainer in the PDO editor and installed **as is** in the demo instance
([ADR-0074 §6](../../../../docs/adr/0074-les-medias-du-readme-sont-joues-par-de-vrais-agents-sur-une-instance-de-demo-isolee.md)).
Only `name:` is set to the demo name. Never touch their layout by hand: `make readme-media-export`,
redraw in the editor, then `make readme-media-import` (see [the tool's README](../../README.md#target-pipelines)).

| File | State | Library name | Scenes |
| --- | --- | --- | --- |
| `implement-review.pipelines.yaml` | built: `Start → implementer → End` | `readme-pipelines` | Visual pipelines |
| `implement-review.yaml` (+ `implement-review.prompts/`) | complete: `implementer → reviewer`, loop on `verdict = fail`, exit on `verdict = pass` | `readme-implement-review` | Routing & loops, hero, every live scene |
| `prod-check.yaml` (+ `prod-check.prompts/`) | the pipeline the `prod-health-check` trigger fires | `prod-check` | Triggers |

The built state takes `implement-review`'s prompts, for its nodes.
