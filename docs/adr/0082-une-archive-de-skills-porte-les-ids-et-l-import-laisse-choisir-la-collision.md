# Une archive de skills porte les ids, et l'import laisse l'utilisateur trancher chaque collision

> Statut : accepted (grilling du 2026-11-05, pdo-issues#2 « Export de skills depuis la skill bank »)

Un skill ne quittait une instance qu'embarqué dans l'export d'une pipeline (Voyage par document, #673). On ajoute l'**Archive de skills** : un zip exporté depuis la banque, réimporté par l'**import de la banque** (une Source de plus, déposée en fichier) plutôt que par l'import de pipeline — c'est le parcours « import de skills » que l'utilisateur connaît déjà.

L'archive est **plate** (`<nom>/SKILL.md` + fichiers) et porte les **ids** dans un **manifeste** à sa racine, pas dans le frontmatter : le contenu du skill n'est pas altéré par le voyage, et l'archive décompressée reste une Source « dossier local » valide. Conserver l'id est ce qui permet à une pipeline importée *plus tard* sur l'instance cible de retrouver les skills importés à la main ; une archive sans manifeste (zip fait main) retombe sur l'import par nom. Le même import accepte le sidecar `<pipeline>.skills.zip`, dont le dossier tient lieu d'id.

Parce que les ids voyagent, une collision se juge **par id** avant le nom, et l'import ne tranche jamais à la place de l'utilisateur : laisser tel quel (défaut), remplacer sur place, déplacer (remplacer puis ranger dans le dossier de l'import), ou dupliquer sous un nouvel id. Le dossier cible existant se fusionne ou se double, au choix. Aucun écrasement sans geste explicite.

## Considered Options

- **Réutiliser la politique du sidecar telle quelle** (id connu conservé, nom suffixé automatiquement) — écartée : pertinente quand le skill accompagne sa pipeline, pas quand l'utilisateur transporte délibérément une *nouvelle version* d'un skill qu'il a déjà.
- **Ids dans le frontmatter (`pdo_id`)** — écartée : modifie le contenu du skill (et son `content_hash`) au passage de la frontière.
- **Recréer la hiérarchie de dossiers** — écartée : la hiérarchie est propre à chaque banque ; l'import range dans un dossier nommé d'après le fichier, comme toute Source.

## Consequences

- Les skills seedés (verrouillés) ne s'exportent pas et ne s'importent pas : chaque instance les seed.
- Hors périmètre : la banque ne mémorise pas le bit exécutable des fichiers de référence ; l'archive reproduit ce que la banque stocke.
