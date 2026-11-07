# Les variables d'une session ne passent par aucune ligne de commande

> Statut : accepted (2026-11-07). Numéro attribué à la fusion vers `main`. Amende ADR-0030 §5
> (le catalogue d'un nœud `script` ne traverse plus l'exec en `-e KEY=VALUE`), ADR-0060 (la
> session ne lance plus `exec bash -c '…'`) et ADR-0081 (`node_env` dans le sandbox).

## Contexte

Le core lançait une session par `tmux new-session … "exec bash -c 'export X='…' && … <tail>'"`
(`wrap_with_env`, `build_hosted_script`). Les valeurs exportées figuraient donc dans l'argv de
`tmux new-session`, du serveur tmux qu'il démarre et du `bash -c` de la session : tout utilisateur
local les lisait avec `ps -eo args` tant que la session vivait. Cela touchait le jeton d'un Node et
toute variable secrète qu'une extension fournit par `node_env` ou `session_env` (jetons d'accès,
clés d'API). Dans un Run sandboxé, le `docker exec` portait en plus chaque variable du Node en
`-e KEY=VALUE`.

## Décision

1. **Le script d'une session voyage dans un fichier privé.** Les constructeurs de script rendent
   un script bash (`export … && <tail>`), sans trampoline `bash -c`. L'adaptateur tmux l'écrit dans
   un fichier `0600`, au nom imprévisible, dans le dossier privé de l'utilisateur
   (`$XDG_RUNTIME_DIR`, sinon le dossier temporaire), jamais dans un dépôt ; tmux reçoit
   `exec bash <chemin>`. Le chemin seul passe en argv.
2. **Le fichier n'existe plus une fois le harnais lancé.** Sa première ligne le supprime : `bash`
   continue de lire par le descripteur qu'il tient. L'adaptateur attend cette suppression (30 s au
   plus) puis supprime le fichier lui-même, ce qui couvre un `tmux new-session` en échec et un
   `bash` qui ne l'a jamais lu : ce lancement-là échoue, sa session est tuée.
3. **Le sandbox transmet les noms, pas les valeurs.** Les variables du Node (catalogue d'un
   script, variables des extensions, env du jeu de staging) sont exportées côté hôte, puis le
   `docker exec` les transmet par leur nom (`-e NAME`), comme il le faisait déjà pour
   `PDO_NODE_ID` et `PDO_NODE_ITER`. `PDO_DAEMON_URL` n'est toujours jamais retransmis.
4. **Le contrat de `node_env` ne change pas.** Les valeurs restent dans l'environnement du harnais
   (`/proc/<pid>/environ`, lisible par le seul même utilisateur) : seule la ligne de commande change.

## Conséquences

- Toutes les sessions passent par le même adaptateur : Node, Retry, reprise, Session hébergée,
  Pipeline Manager, assistant de bibliothèque, shell de Run.
- Un lancement attend que `bash` ait lu son fichier, quelques millisecondes en temps normal.
- Dans un sandbox, les variables du Node sont aussi dans l'environnement du client `docker` côté
  hôte : un nom `DOCKER_*` fourni par une extension le pilote (autre démon, autre contexte).
- Le marqueur de session du sandbox (`PDO_SBX_SESSION=<nom de session>`) reste valué sur l'exec :
  ce n'est pas un secret, et le nom de session figure déjà dans l'argv de tmux (`-s`).
- Écarté : `tmux new-session -e NAME=value`, qui met aussi la valeur dans l'argv de tmux.
