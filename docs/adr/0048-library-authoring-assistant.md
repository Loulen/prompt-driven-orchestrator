# Assistant d'authoring de bibliothèque — copilote design-time des templates

Sans cet ADR, un agent traiterait l'assistant de bibliothèque comme un Pipeline Manager (terme
load-bearing, attaché à un Run et émetteur de commandes), et le câblerait avec un MCP custom ou une
écriture à chaque édition.

> Statut : **accepted** (#302).
>
> **Amendé par ADR-0051 (#594).** Les décisions **1** (session keyée sur l'id de la pipeline), **3**
> (create-on-open / reap-on-leave sur l'onglet) et **4** (jamais reapée par le sweep, sans TTL) ne
> valent plus : il y a un **seul** assistant par daemon, la pipeline courante arrive par le **focus**
> à chaque message, et le sweep reprend la main dès que l'humain n'est plus là. Le reste tient :
> mécanisme de session (§2), write-on-save (§6), prompt primé sans MCP custom (§5), accès unifié par
> la toolbar (§7). Un détail de §5/§6 change : **l'endpoint de persistance** n'est plus
> `POST /library/pipelines` mais `POST /sessions/libassist/save`, qui écrit dans le fichier que le
> focus désigne.

Écrire une pipeline **template** se faisait à la main (canvas ou YAML). #302 ajoute un **copilote
d'authoring** : une session `claude` inline, ouverte dans le dossier des templates, à qui
l'utilisateur **décrit** le changement ; l'agent produit le YAML (+ prompts par nœud), l'humain
relit, l'agent écrit.

## Terminologie (F1) — « assistant », pas « manager »

*Manager* est load-bearing (CONTEXT §*Pipeline Manager*) — REPL **attachée à un Run**, dont toute la
valeur est `POST /runs/<id>/commands`. L'assistant n'est attaché à **aucun Run**, n'émet **aucune**
commande, et son seul effet est **d'écrire des fichiers** template. Il réutilise le **mécanisme**
d'ADR-0021 (session inline + pont PTY), pas la **sémantique** du Manager.

## Fit philosophique

Aucun conflit avec ADR-0001/0002 (« pas de LLM-router **runtime** ») : l'LLM agit au **design-time**
et produit un YAML relu par l'humain. Cela **renforce** *Deliberate, then autonomous* (ADR-0012).

## Ce qu'on décide

1. *(remplacée par ADR-0051)* Nouvelle classe de session `pdo-libassist-*`, cwd = le dossier des
   templates du scope. Tail = `claude` (`SessionTail::Agent`), **pas** `bash -i` : c'est une REPL.

2. **Endpoint create-if-absent + endpoint de reap.** `POST /sessions/{id}/libassist?scope=<scope>`
   garantit l'existence de la session et renvoie son nom ; l'attache se fait par le pont PTY existant
   (`WS /sessions/<name>/pty`, générique — zéro code backend d'attache). Idempotence race-free
   (create-then-verify-on-failure). Un `DELETE` reap la session — c'est le point net-new : aucun
   autre reap n'était piloté par le client.

3. *(remplacée par ADR-0051)* Create-on-open / reap-on-leave sur l'onglet. `claude` **ne sort pas sur
   EOF** (contrairement à `bash -i`, ADR-0021 #4), donc la session survit à une coupure WS
   transitoire — la fermeture est **explicite**, jamais un effet de bord du pont PTY.

4. *(remplacée par ADR-0051)* Jamais reapée par le sweep, jamais de TTL. Ce qui survit : sans le
   préfixe `libassist-` dans `parse_session_name`, le sweep lirait un **nom inconnu** et tuerait la
   session en < 60 s (pinné par un test). Comme le manager/shell, elle est **exempte du cap**
   d'admission.

5. **Prompt système primé.** Le préambule runtime pose l'id de la pipeline, l'URL du daemon et les
   endpoints que l'assistant pilote (valider, persister) ; le rôle statique décrit le **format YAML**
   des pipelines. Même discipline que le manager : on possède le prompt, on documente les endpoints,
   **pas de MCP custom**.

6. **Écrire au save, jamais à chaque édition (F2).** C'est une consigne du prompt (décrire le
   changement, **montrer un diff**, écrire **sur OK**), pas une porte dans le daemon : l'assistant
   valide puis persiste tout le template d'un coup, et le canvas relit sur sauvegarde (store
   disk-first, ADR-0007). Jamais un YAML à moitié édité.

7. **Généralisation au canvas run = même chemin d'accès, pas de nouveau panneau (F4).** L'accès est
   unifié par l'**icône dans la toolbar** : côté bibliothèque elle ouvre l'onglet **Assistant** ;
   côté run l'onglet adéquat reste le **Manager** (déjà auto-spawné). Le net-new est 100 % côté
   bibliothèque.

## Alternatives écartées

- **Partager l'onglet/terme « Manager »** : brouille un terme load-bearing (F1) — objet réel
  différent (pas de Run, pas de `run_command`, effet = écrire un fichier).
- **Tail `bash -i` dans une boucle de respawn** : `claude` ne sort pas sur EOF, donc pas de boucle ;
  et on veut une REPL, pas un bash brut.
- **Auto-spawn à la création de la pipeline (comme le manager de run)** : trop tôt et coûteux ; le
  besoin intuite le cycle de vie du **Shell** (« uniquement sur clic »).
- **Écrire à chaque édition** : le diff-then-save protège un canvas ouvert sur la même pipeline d'un
  écrasement surprise.

## Limites acceptées

- **Réconciliation canvas ↔ fichier (F2)** : l'assistant écrit le fichier ; le canvas relit sur save
  via le store disk-first. Un conflit avec des edits canvas non sauvés reste géré par les mécanismes
  existants (watcher `PipelineModified`, étoile synced/diverged) — pas de verrou d'écriture ajouté.
- **cwd = dossier des templates** : l'assistant y voit toutes les templates du scope (utile comme
  exemples few-shot) ; le primer y est **volontairement pas** écrit (il va dans un `.libassist/`
  frère) pour ne pas polluer le dossier visible.

## Relations

- **ADR-0021** — pont PTY et attache inline verbatim ; endpoint create-if-absent calqué sur
  `open_run_shell`.
- **ADR-0005** — terminal inline xterm.js primaire.
- **ADR-0009 / ADR-0012** — opération atomique side-effect-light qui ne réentre jamais le scheduler.
- **ADR-0001 / ADR-0002** — LLM au design-time, pas de router runtime.
- **ADR-0007** — le store disk-first ; le canvas relit la template sur save.
