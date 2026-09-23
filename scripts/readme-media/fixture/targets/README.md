# Target pipelines

Drawn by the maintainer in the PDO editor and installed **as is** in the demo instance
([ADR-0074 §6](../../../../docs/adr/0074-les-medias-du-readme-sont-joues-par-de-vrais-agents-sur-une-instance-de-demo-isolee.md)).
Only `name:` is set to the demo name. Never touch their layout by hand: redraw it in the editor, then import.

| File | State | Scenes |
| --- | --- | --- |
| `implement-review.pipelines.yaml` | built: `Start → implementer → End` | Visual pipelines |
| `implement-review.yaml` | complete: `implementer → reviewer`, loop on `verdict = fail`, exit on `verdict = pass` | Routing & loops, hero, every live scene |
| `prod-check.yaml` (+ `prod-check.prompts/`) | the pipeline the `prod-health-check` trigger fires | Triggers |

`implement-review`'s prompts are the ones in `../pipelines/implement-review.prompts/`.
Not wired yet: the scenes still read `../pipelines/` until the #852 rework lands.
