# Les ports de sortie sont des répertoires, pas des fichiers

CONTEXT.md définit le schéma d'adressage des artefacts comme `<artifacts>/<node-id>/iter-<N>/<port-name>.md` — un fichier unique par port de sortie. Ce modèle ne supporte que du texte markdown. L'ajout d'images comme type de sortie (screenshots Chrome MCP, captures UI, etc.) nécessite qu'un port puisse contenir plusieurs fichiers de types hétérogènes, et le pattern markdown-avec-références-inline ne suffit pas car on veut aussi des ports de type image pur (sans markdown).

**Décision : chaque port de sortie mappe vers un répertoire, pas un fichier.**

```
<artifacts>/<node-id>/iter-<N>/<port-name>/
```

Le contenu du répertoire dépend du type du port :

- `markdown` : le répertoire contient un fichier markdown canonique (ex. `output.md`) avec frontmatter YAML. Il peut aussi contenir des images référencées inline par le markdown (`![desc](./screenshot.png)`). L'UI rend les images inline lors de la visualisation de l'artefact.
- `image` : le répertoire contient un unique fichier image.
- `image_list` : le répertoire contient un ou plusieurs fichiers image. L'ordre est alphabétique par nom de fichier.

La validation à la complétion (déjà décrite dans CONTEXT.md, section "Validation à la complétion + fallback tmux") s'adapte :

- Port `markdown` : le répertoire existe, contient au moins un `.md`, la frontmatter de ce `.md` respecte le schéma déclaré.
- Port `image` : le répertoire existe, contient exactement un fichier image (extensions reconnues : `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`).
- Port `image_list` : le répertoire existe, contient au moins un fichier image.

Le prompt augmenter (préambule runtime) adapte ses instructions au type du port :

- `markdown` : "écris ton artefact à `<dir>/output.md` avec frontmatter YAML contenant...". Inchangé sémantiquement, juste un chemin plus profond.
- `image` / `image_list` : "dépose tes images dans `<dir>/`. Chaque fichier image du répertoire sera considéré comme un output."

La résolution des inputs downstream suit : le resolver lit le contenu du répertoire du port source, et le prompt augmenter passe les chemins des fichiers trouvés au nœud consommateur.

**Pourquoi.** Choisi contre l'alternative *"fichier unique par port, les images sont des fichiers séparés à côté du markdown avec une convention de nommage"* parce que ça crée deux modèles d'adressage (fichier pour markdown, glob informel pour images) et ne permet pas de typer un port comme `image` sans markdown compagnon. Choisi contre l'alternative *"fichier manifeste YAML listant les contenus du port"* parce que c'est une indirection supplémentaire que l'agent doit produire correctement — un répertoire avec un glob est plus simple à écrire et à valider. Le coût est la migration de tous les chemins existants (`<port>.md` → `<port>/output.md`) dans le prompt augmenter, le blackboard resolver, le scheduler, et le frontend — migration mécanique, pas de changement sémantique.
