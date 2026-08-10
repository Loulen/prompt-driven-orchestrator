# ADR-0039 — Le daemon reste un crate unique, les modules restent frères, lib.rs est carvé par concern, jamais par répertoire

> Statut : accepted (issue #494, portée S0-S2 du grilling du 2026-07-30, re-vérifiée par sondes le
> 2026-08-05 sur `8b5755d`). Vocabulaire : `docs/agents/module-layout.md` (« module layout », jamais
> « layout » nu — ce dernier désigne le partitionnement layout/sémantique des nœuds dans CONTEXT.md).
> **Amende ADR-0009** : les trois couches (primitives pures, orchestration, bord) sont une taxonomie
> de *rôles*, pas de *fichiers* ; le présent ADR refuse qu'elles deviennent des répertoires.
> **Ne touche pas** au découpage frontend en 8 dossiers (inspector/editor/canvas/…) : ce déplacement
> est S3/S4, derrière un gate humain, bloqué par #338 et #359 — cet ADR ne le tranche pas, il refuse
> seulement de le préempter par une taxonomie de noms.

## Contexte

#494 part d'un constat vrai : `crates/pdo-daemon/src/` est une liste plate de ~54 fichiers,
`frontend/src/components/` une liste plate de ~137. Le réflexe — « ranger selon les bonnes
pratiques » — est de créer des répertoires (`sandbox/`, `triggers/`, `scheduler/`…) ou de scinder le
crate en sous-crates par couche d'ADR-0009. Cet ADR consigne **pourquoi on ne le fait pas**, pour que
la question ne se rejoue pas à chaque nouveau fichier.

Le refus n'est pas « la platitude est belle ». C'est que, pour *ce* daemon, le chemin de module porte
des contrats externes, et qu'une taxonomie par nom est réfutée par le graphe d'imports réel.

## Ce qu'on refuse

### 1. Pas de sous-crates. Un seul crate `pdo-daemon`, workspace mono-membre

Scinder par couche (un crate `primitives`, un crate `orchestration`) achète des frontières de
compilation que rien ne réclame : il n'existe **aucun** second consommateur des types du daemon. La
seule frontière externe réelle est `crates/pdo-daemon/tests/`, qui consomme une poignée d'items
(`serve_with_config`, `DaemonConfig`, `DaemonHandle`, et trois modules : `admission`,
`stale_detector`, `tmux_session_manager`). Cette surface se déclare item par item ; elle ne justifie
pas un découpage de crate. Un split imposerait `pub` sur tout ce qui traverse une frontière interne —
soit l'inverse exact du resserrage que #494 vient de faire (194 + ~90 items ramenés à `pub(crate)`).

### 2. Pas de répertoires dans `crates/pdo-daemon/src/`

Les cibles `tracing` sont `module_path!()`. Déplacer `trigger_store.rs` sous `triggers/` réécrit sa
cible de `pdo_daemon::trigger_store` en `pdo_daemon::triggers::trigger_store`, ce qui casse **en
silence** tout `RUST_LOG=pdo_daemon::trigger_store=debug` et tous les greps de runbook qui filtrent
sur le chemin. Le chemin de module est ici une **interface opérationnelle**, pas un détail de
rangement. Aucun `.rs` n'étant déplacé par #494, `module_path!()` ne bouge pas : c'est un non-sujet
*aujourd'hui*, et la raison majeure de refuser qu'il le devienne demain.

### 3. Les trois couches d'ADR-0009 ne reçoivent pas de dossiers

« Primitive pure / orchestration / bord » est un rôle qu'un module joue, lisible dans sa signature
(pas de tmux, pas de DB, pas d'horloge pour la couche 1). Le matérialiser en `primitives/`,
`orchestration/`, `io/` obligerait à trancher le dossier de chaque module hybride, rouvrirait le
problème du §2, et n'ajouterait rien qu'un `grep` sur les signatures ne donne déjà.

## Pourquoi c'est tenable sans dossiers

Le chemin de module **n'est pas un contrat public** : hors des trois modules consommés par `tests/`,
tout est `pub(crate)` ou `mod` privé, donc renommer ou fusionner un module ne casse aucun appelant
externe. Ce qui garde la platitude *saine* n'est pas un dossier, c'est un couple lint + ratchet :

- `#![warn(unreachable_pub)]` en tête de `lib.rs`, combiné au `clippy -D warnings` de la CI :
  tout `pub` sans consommateur externe redevient rouge. La surface ne peut plus s'élargir par
  inadvertance.
- `scripts/layout-ratchet.sh` (gating dans le job `frontend`) : le nombre de fichiers directs de
  chaque dossier surveillé ne peut plus croître au-delà de sa baseline. Un nouveau fichier top-level
  est un acte délibéré, justifié en PR ; un rangement fait baisser la baseline dans le même commit.

La règle d'usage (« un concern = un fichier frère ») vit dans `docs/agents/module-layout.md`. Elle
**ne qualifie pas** en ADR : c'est une convention de style, révisable sans cérémonie. Seul le *refus*
ci-dessus qualifie, parce qu'il ferme des portes structurelles (sous-crates, répertoires) dont la
réouverture coûte cher.

## Alternatives écartées

- **Taxonomie par préfixe de nom** (« tous les `*_store` ensemble », « tous les `Port*` ensemble »).
  Réfutée par le graphe d'imports, pas par goût : côté daemon, les familles de préfixe n'ont pas de
  cohésion d'appels ; côté frontend, la famille `Port*` a une intersection d'imports vide et les 16
  composants `*Modal` ne partagent qu'**une** arête interne. Regrouper par nom rangerait des fichiers
  qui ne se parlent pas et séparerait des fichiers qui se parlent — l'inverse d'un module.
- **Un plafond fixe de fichiers** (« max 30 entrées par dossier »). Écarté au grilling : un seuil
  absolu est un bikeshed (pourquoi 30 ?) et se traite en une fois puis dérive. Le ratchet encode la
  seule propriété qui compte — *ne pas grossir, et rétrécir quand on range* — sans nombre magique.
- **Le découpage frontend en 8 dossiers** (inspector/editor/canvas/runs/shell/artifacts/dialogs/
  shared). Ce n'est **pas** refusé ici : c'est reporté. Il déplace des fichiers (donc gate humain,
  ADR-0012 sur les worktrees, seuil de rename git) et dépend de #338 (touche `SettingsModal` +
  `NewRunModal` + `lib.rs`) et #359 (réécrit les 4 plus gros composants). `components/ui/` reste la
  seule exception de dossier, et seulement parce que `npx shadcn add` ré-aplatit tout le reste.

## Limites acceptées

- **La platitude reste inconfortable à parcourir à l'œil.** L'ADR l'assume : le confort de navigation
  d'un humain qui scanne un `ls` ne vaut pas la casse d'une cible `tracing` ni un `pub` de fuite. Les
  outils (grep, go-to-definition, ratchet) portent la navigabilité, pas l'arborescence.
- **Le ratchet ne juge que le *nombre*, pas la *cohésion*.** Un fichier peut être mal placé sans faire
  bouger le compte. La règle d'usage et la revue restent le garde-fou qualitatif ; le ratchet n'est
  que le cliquet quantitatif.
- **#494 ne range rien de la liste frontend.** Il supprime des fichiers morts et fige les baselines ;
  le vrai rangement (S3/S4) attend son gate. Cet ADR est ce qui empêche ce report de dériver en
  « on rangera par dossiers plus tard ».

## Relations

- **ADR-0009** (primitives à trois couches) — amendée : les couches sont des rôles, jamais des
  répertoires.
- **ADR-0012(a)** (le balayage ne touche ni worktree ni branche) — invoquée : tout `git mv` massif
  relève d'un déplacement gated, hors périmètre S0-S2.
- **ADR-0016** (import de workflows via AST) — inchangée : `oxc_*` reste épinglé, sans rapport avec le
  layout des modules.
