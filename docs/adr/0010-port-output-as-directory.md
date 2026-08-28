# Les ports de sortie sont des répertoires, pas des fichiers

**Un port de sortie mappe vers un répertoire** `<artifacts>/<node-id>/iter-<N>/<port-name>/`, jamais vers un fichier unique : sans ça on retombe sur deux modèles d'adressage (un fichier pour markdown, un glob informel pour les images) et on ne peut pas typer un port `image` sans markdown compagnon.

Contenu selon le type du port : `markdown` (un fichier canonique `output.md` à frontmatter YAML, plus d'éventuelles images référencées inline), `image` (exactement un fichier), `image_list` (une ou plusieurs images — **l'ordre est alphabétique par nom de fichier, c'est un contrat d'auteur**).

Écarté : un **manifeste YAML** listant le contenu du port — une indirection de plus que l'agent doit produire correctement, là où un répertoire + glob s'écrit et se valide sans intermédiaire.
