# La provenance parent/enfant est mécanique et la liaison de tout nœud parent est forte

L'orchestration récursive (#709) exige qu'un Run sache qui l'a créé et qu'un nœud ne se déclare jamais terminé pendant que ses Runs enfants tournent. Décision double : le lien `parent_run_id` + `parent_node_id` est **posé par le daemon** pour tout Run créé depuis une session de nœud (agent, script), jamais déclaré par l'appelant, qui ne peut pas le fournir hors session ; et **tout nœud parent** (nœud qui a au moins un Run enfant, toggle ou non) reste en attente jusqu'à ce que tous ses enfants soient terminaux, sa complétion refusée entre-temps (`children_pending`) quelle qu'en soit la source : `pdo complete`, auto-complétion de fin de tour, fin d'un `script`. Le watcher de la liaison complète le nœud quand tous ses enfants sont terminaux sans échec ; un enfant `failed` parque le nœud en attente utilisateur. Le refus dit lui-même à l'agent quoi faire (`pdo run wait`), puisqu'un nœud sans toggle n'a pas le skill. L'UI d'orchestration (compteurs d'enfants, onglet *Orchestration*) suit de même les enfants réels. Le toggle « Orchestrator » n'ajoute que le skill `pdo-orchestrate` et sa ligne d'invocation, et annonce le nœud parent (onglet affiché dès le départ). Un enfant est partout ailleurs un Run ordinaire : aucune propagation de mort vers le bas (stop, archive ou forget du parent ne le touchent), Projet du parent par défaut mais modifiable, isolation de worktree inchangée (ADR-0060).

## Considered Options

- **Type de Node `orchestrator` dédié** : rejeté. La capacité de lancer des Runs est universelle (tout nœud peut orchestrer, avec ou sans le skill) ; un type dédié en ferait un clergé spécialisé.
- **Liaison et UI portées par le toggle** : rejetée. Un nœud sans toggle qui lançait un Run n'en montrait rien et se complétait pendant que l'enfant tournait, alors que le Run enfant existait bien. Le toggle dit une intention ; la filiation est le fait.
- **Liaison faible** (l'agent complète quand il veut, PDO ne fait qu'afficher) : rejetée. Sans liaison, la complétion du nœud ment dès qu'un enfant échoue après `pdo complete`. Le refus de complétion tant que des enfants tournent est le contrat qui rend l'orchestration fiable.
- **Liaison réservée aux nœuds agentiques** : rejetée. Une seule règle pour tous les nœuds ; le watcher complète un `script` déjà sorti exactement comme un agent parqué.
- **Provenance déclarée** (champ `parent` sur `POST /runs`, comme `triggered_by`) : rejetée. Un agent pourrait mentir ou se rattacher au Run voisin ; la session est le seul témoin digne de foi, et le daemon la connaît déjà.

## Consequences

- La projection d'événements porte `parent_run_id` et `parent_node_id` dès `RunStarted` ; la liste de Runs les expose pour l'arbre de Runs.
- Le seed du skill `pdo-orchestrate` dans la Banque de skills est le premier mécanisme de seed du produit : contenu versionné avec PDO, dossier « PDO », verrouillé en édition et suppression, ressemé au changement de version.
- Le scheduler a une dépendance NodeRun → Runs enfants pour **tout** nœud : un nœud parent peut attendre indéfiniment des enfants `awaiting_user`, c'est voulu (l'utilisateur tranche).
- Pas de migration : un nœud sans toggle déjà lancé, aux enfants vivants, est retenu dès la mise à jour du daemon.
- Le genre de nœud *Orchestrator* des Stats reste lu sur le toggle, pas sur la filiation.
