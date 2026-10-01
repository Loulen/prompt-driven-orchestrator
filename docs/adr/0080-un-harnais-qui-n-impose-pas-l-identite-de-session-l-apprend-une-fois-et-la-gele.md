# ADR-0080 — Un harnais qui n'impose pas l'identité de session l'**apprend** une fois et la gèle ; la reprise est toujours par cet id, jamais par « la dernière »

Sans cet ADR, un agent brancherait `vibe` comme `opencode` (résolution par répertoire de travail à
chaque lecture, reprise par `-c`) et un nœud repris rouvrirait la session d'un **autre** nœud du même
worktree, avec son coût.

> Statut : accepted (grilling du 2026-09-30, story #960 « `vibe`, cinquième harnais first-party »).
> Vocabulaire : CONTEXT.md § *Harnais agentique*, terme **identité de session apprise**. **Précise
> ADR-0045 §Limites** : « attribution par répertoire de travail » y désignait le repli d'`opencode`
> (aucune lecture) ; ici on lit, donc on doit désigner *une* session, une fois.

## Contexte

Mesuré sur `vibe 2.25.8` : pas de `--session-id`. Chaque session vit dans
`$VIBE_HOME/logs/session/session_<ts>_<id8>/` avec `meta.json` (`session_id` en UUID complet,
`start_time`, `environment.working_directory`, `stats`, `config.active_model`) et `messages.jsonl`.
La reprise est `--resume <SESSION_ID>` ou `-c`. Deux détails du code de `vibe` tranchent :

- `-c` reprend la session dont `messages.jsonl` a le **`mtime` le plus récent**, sans filtre de cwd :
  une session longue d'un autre nœud, encore active, gagne toujours.
- Une session n'est reprisable que si `meta.json` **et** `messages.jsonl` existent et sont du JSON
  valide ; un nœud tué entre deux écritures laisse un dossier non reprisable.

## Ce qu'on décide

1. **Trois modes d'identité, pas deux.** *Imposée* (`claude`, `copilot`, `pi` : PDO nomme la session
   avant qu'elle existe), *apprise* (`vibe` : le harnais nomme, PDO lit le nom **une fois**), *aucune*
   (`opencode` : descripteur sans lecture). Le mode est une capacité déclarée par harnais (ADR-0051).
2. **Apprentissage : la première session du cwd créée après le spawn.** PDO note l'heure de spawn ;
   la résolution balaie les `meta.json` du store et retient la session dont `working_directory` est
   le cwd du nœud et `start_time >= spawn`, la plus ancienne qui satisfait les deux. L'UUID complet de
   `meta.json` (pas les 8 caractères du dossier) est **gelé dans l'event log du nœud** ; on ne
   rebalaie plus.
3. **Toutes les lectures passent par l'id gelé** : coût, modèle observé, contexte, pilotage, fin de
   tour. Avant l'apprentissage, elles répondent « — » (absence dite, ADR-0045), jamais une autre
   session.
4. **La reprise est par identité apprise** (`--resume <uuid>`), jamais `-c`. Sans id appris, ou si
   la session n'est pas reprisable (fichier manquant ou invalide), la reprise **relance une session
   neuve** et le dit ; le nouvel id remplace l'ancien à l'apprentissage suivant.

## Les alternatives écartées

**Résoudre par cwd à chaque lecture, reprendre par `-c`** (la forme d'`opencode`). Le coût d'un nœud
saute de session en session, et la reprise rouvre la session la plus bavarde du worktree.

**Un store par nœud via `VIBE_HOME`.** Isolerait parfaitement les sessions, mais `VIBE_HOME` déplace
**tout** (clé, `config.toml`, confiance, catalogue) : hors sandbox, PDO ne met en scène le home
d'aucun harnais (prérequis, CONTEXT.md). Reste la bonne réponse **dans** le sandbox, où le home est
déjà mis en scène (ADR-0063).

**Demander à `vibe` un `--session-id`.** Souhaitable, hors de notre main ; le jour où il existe, le
harnais passe en identité imposée et cet ADR ne le concerne plus.

## Ouverture non construite ici

Le hook `post_agent` de `vibe` reçoit `session_id` sur stdin. Quand `autocomplete_turn_end` est coché,
ce hook peut **confirmer** l'id appris par balayage, ou le corriger si deux nœuds se sont croisés. Un
apprentissage *par hook* serait plus sûr que par balayage, mais dépendrait d'un réglage décoché par
défaut ; on garde le balayage comme mécanisme de base et on note l'amélioration.

## Limites acceptées

- **Deux nœuds `vibe` spawnés dans le même worktree à la même seconde** peuvent apprendre la même
  session ou s'échanger la leur. Réduit par le filtre `start_time >= spawn`, pas supprimé.
- **Une session dont le premier tour n'a pas commencé** n'a pas encore de dossier : l'apprentissage
  est différé, les lectures disent « — » jusque-là.
- **L'apprentissage court à deux moments** : au balayage (toute itération `vibe` d'un Run
  vivant, quel que soit son statut) et **à la complétion du nœud** (#963 : un nœud que son hook
  de fin de tour complète sept secondes après le spawn serait sinon jamais costé, mesuré au FP).
  Un nœud tué sans complétion et dont le Run se termine avant le balayage suivant reste la
  seule fenêtre aveugle.
- **Le répertoire de travail se compare en forme canonique** : `vibe` enregistre le chemin
  résolu par le noyau (`/private/tmp/…` sur macOS) là où PDO tient `/tmp/…`.
- **Une horloge de machine qui recule** entre spawn et premier tour rend le filtre aveugle ; on
  n'ajoute pas de tolérance : le cas est une panne, pas un usage.

## Antériorité

ADR-0045 (limites, `opencode`), ADR-0051 (une capacité est un dispatch), ADR-0007 (gelé au spawn,
lu depuis l'event log), ADR-0063 (staging sandbox), #702 (identité imposée de `pi`), #960.
