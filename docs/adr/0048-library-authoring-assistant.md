# Assistant d'authoring de bibliothèque — copilote design-time des templates

> Statut : **accepted** (#302).
>
> **Amendé par ADR-0051 (#594).** Les décisions **1** (session keyée sur l'id de la pipeline), **3**
> (create-on-open / reap-on-leave sur l'onglet) et **4** (jamais reapée par le sweep, sans TTL) ne
> valent plus : il y a un **seul** assistant par daemon, la pipeline courante arrive par le **focus**
> à chaque message, et le sweep reprend la main sur la session dès que l'humain n'est plus là. Le
> reste tient : mécanisme de session (§2), write-on-save (§6), prompt primé sans MCP custom (§5),
> accès unifié par la toolbar (§7). Un détail de §5/§6 change tout de même : **l'endpoint de
> persistance** n'est plus `POST /library/pipelines` mais `POST /sessions/libassist/save`, qui écrit
> dans le fichier que le focus désigne — voir les conséquences d'ADR-0051.

Écrire ou modifier une pipeline **template** se faisait à la main : câbler nodes / edges / prompts
sur le canvas, ou éditer le YAML. #302 ajoute un **copilote d'authoring** : une session `claude`
inline, ouverte dans le dossier des templates, à qui l'utilisateur **décrit** le changement en
langage naturel ; l'agent produit le YAML (+ les prompts par nœud), l'humain relit, l'agent écrit.

C'est une **nouvelle classe de session** et une **nouvelle modalité d'authoring** — d'où cet ADR,
adossé à ADR-0021 (mécanisme de session), ADR-0005 (terminal inline), ADR-0009/0012 (aucun effet
durable initié par le *runtime*, pas de ré-entrée scheduler) et ADR-0001/0002 (LLM design-time OK).

## Terminologie (F1) — « assistant », pas « manager »

L'issue disait « partager l'onglet Manager ». Refusé : *manager* est load-bearing (CONTEXT §*Pipeline
Manager*) — REPL **attachée à un Run**, dont toute la valeur est `POST /runs/<id>/commands`. L'assistant
n'est attaché à **aucun Run**, n'émet **aucune** commande, et son seul effet est **d'écrire des
fichiers** template. Terme et concept distincts : **assistant de bibliothèque / copilote pipeline**.
Il réutilise le **mécanisme** d'ADR-0021 (session inline + pont PTY), pas la **sémantique** du Manager.

## Fit philosophique

Aucun conflit avec ADR-0001/0002 (« pas de LLM-router **runtime** ») : l'LLM agit au **design-time** et
produit un YAML relu par l'humain — l'inverse d'un routeur d'exécution. Cela **renforce** *Deliberate,
then autonomous* (ADR-0012) : la valeur de PDO est dans le temps de conception ; un copilote amplifie
exactement cette phase.

## Ce qu'on décide

1. **Nouvelle classe de session `pdo-libassist-<pipeline-id>`.** Keyée sur l'**id de la pipeline**
   (l'ownership, F3), pas sur un Run. cwd = le dossier des templates du scope
   (`~/.pdo/library/pipelines/` pour `user`/`library`, `<repo>/.pdo/library/pipelines/` pour `repo`).
   Tail = `claude` (comme le manager, `SessionTail::Agent`), **pas** `bash -i` : c'est une REPL, pas un
   shell brut.

2. **Endpoint create-if-absent + endpoint de reap.** `POST /sessions/{id}/libassist?scope=<scope>`
   garantit l'existence de la session et renvoie son nom ; l'attache se fait ensuite par le pont PTY
   existant (`WS /sessions/<name>/pty`, générique — zéro code backend d'attache). Idempotence race-free
   (create-then-verify-on-failure), copie conforme de `open_run_shell`. Un `DELETE /sessions/{id}/libassist`
   **reap** la session — c'est le point net-new : aucun autre reap n'était piloté par le client.

3. **Cycle de vie = create-on-open / reap-on-leave (F3).** Le propriétaire l'a tranché : « dès qu'on
   quitte l'onglet on reap la session ». Le front spawn la session à l'ouverture de l'onglet Assistant
   et la reap au démontage (changement d'onglet, fermeture du panneau, changement de pipeline). Une
   ré-ouverture ré-attache la **même** session (create-if-absent) ; c'est robuste sans état côté front.
   `claude` **ne sort pas sur EOF** (contrairement à `bash -i`, ADR-0021 #4), donc la session survit à
   une coupure WS transitoire — la fermeture est **explicite**, jamais un effet de bord du pont PTY.

4. **Jamais reapée par le sweep d'orphelins, jamais de TTL.** Une session `pdo-libassist-*` n'a **pas de
   Run** sur quoi keyer un verdict *absent* / *archived* / *stale*. Le sweep la **garde
   inconditionnellement** (`decide_one` → `Keep`) : elle est reapée uniquement par le `DELETE` explicite.
   Sans le préfixe `libassist-` dans `parse_session_name`, le sweep l'aurait lue comme un **nom
   inconnu** et tuée en < 60 s (le piège exact que la branche de parse évite — pinné par un test).
   Comme le manager/shell, elle est **exempte du cap** d'admission (ce n'est pas un nœud projeté).

5. **Prompt système primé (demande explicite du propriétaire).** Le préambule runtime pose l'id de la
   pipeline, l'URL du daemon et les deux endpoints que l'assistant pilote — `POST /nodes/parse`
   (valider) et `POST /library/pipelines` (persister) — avec exemples `curl` ; le rôle statique
   (`prompts/builtin/library-assistant.md`) décrit le **format YAML** des pipelines et la disposition
   `<id>.prompts/`. Même discipline que le manager : on possède le prompt, on documente les endpoints,
   **pas de MCP custom**.

6. **Écrire au save, jamais à chaque édition (F2).** Le propriétaire : « on écrit au save uniquement ».
   C'est une consigne du prompt (décrire le changement, **montrer un diff**, écrire **sur OK**), pas une
   porte dans le daemon : l'assistant valide via `/nodes/parse` puis persiste tout le template via
   `POST /library/pipelines`, et le canvas relit sur sauvegarde (store disk-first, ADR-0007). Écrire le
   fichier entier d'un coup — jamais un YAML à moitié édité.

7. **Généralisation au canvas run = même chemin d'accès, pas de nouveau panneau (F4).** Le propriétaire
   a préféré bundler plutôt que trimballer un slice trivial. L'accès est unifié par l'**icône dans la
   toolbar** (glyphe « agent » à côté du `(i)`) : côté bibliothèque elle ouvre l'onglet **Assistant** ;
   côté run l'onglet adéquat reste le **Manager** (déjà auto-spawné). Le net-new est **100 % côté
   bibliothèque** ; le côté run réutilise le Manager existant.

## Alternatives écartées

- **Partager l'onglet/terme « Manager »** : brouille un terme load-bearing (F1) — objet réel différent
  (pas de Run, pas de `run_command`, effet = écrire un fichier).
- **Tail `bash -i` dans une boucle de respawn (comme le shell d'ADR-0021)** : inutile ici — `claude` ne
  sort pas sur EOF, donc pas de boucle à écrire ; et on veut une REPL, pas un bash brut.
- **Reap par TTL / par le sweep** : un outil interactif ne doit pas être arraché ; et sans Run il n'y a
  pas de signal d'absence à keyer. Le reap explicite du front est plus simple et plus véridique.
- **Auto-spawn à la création de la pipeline (comme le manager de run)** : trop tôt et coûteux. Le besoin
  a bien intuité le cycle de vie du **Shell** (« uniquement sur clic »), pas l'auto-spawn du Manager.
- **Écrire à chaque édition** : le propriétaire l'a jugé trop tôt (F2) — le diff-then-save protège un
  canvas ouvert sur la même pipeline d'un écrasement surprise.

## Limites acceptées

- **Fuite au redémarrage-du-daemon-onglet-fermé.** Le sweep gardant toujours la session, une session
  dont l'onglet a été fermé **exactement pendant** un redémarrage du daemon n'est pas reapée. Coût
  négligeable (un `claude` inactif, exempt du cap ; CCR désactivé ⇒ aucun trafic tant qu'on ne lui parle
  pas), et auto-guérissant : ré-ouvrir la pipeline ré-attache la session, la quitter la reap.
- **Réconciliation canvas ↔ fichier (F2)** : l'assistant écrit le fichier ; le canvas relit sur save via
  le store disk-first (ADR-0007). Un conflit avec des edits canvas non sauvés reste géré par les
  mécanismes existants (watcher `PipelineModified`, étoile synced/diverged) — pas de verrou d'écriture
  ajouté dans ce MVP.
- **cwd = dossier des templates** : l'assistant y voit toutes les templates du scope (utile comme
  exemples few-shot) ; le primer y est **volontairement pas** écrit (il va dans un `.libassist/` frère)
  pour ne pas polluer le dossier visible.

## Relations

- **ADR-0021** — réutilise le pont PTY et l'attache inline verbatim ; endpoint create-if-absent calqué
  sur `open_run_shell` ; le reap explicite est le pendant lifecycle côté assistant (vs reap sur
  archive/absence côté shell).
- **ADR-0005** — terminal inline xterm.js primaire ; l'assistant s'attache par la même route WS.
- **ADR-0009 / ADR-0012** — opération atomique side-effect-light qui ne réentre jamais le scheduler ;
  surface pilotée par l'humain, autonomie **dans la conception**.
- **ADR-0001 / ADR-0002** — LLM au design-time, pas de router runtime.
- **ADR-0007** — le store disk-first ; le canvas relit la template sur save.
