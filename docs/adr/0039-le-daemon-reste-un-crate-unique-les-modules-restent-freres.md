# ADR-0039 — Le daemon reste un crate unique, les modules restent frères, lib.rs est carvé par concern, jamais par répertoire

Sans cet ADR, un agent « rangerait » la liste plate de `crates/pdo-daemon/src/` en répertoires
(`sandbox/`, `triggers/`…) ou en sous-crates par couche d'ADR-0009 — ce qui casse en silence les
cibles `tracing` et force des `pub` de fuite.

> Statut : accepted (issue #494, portée S0-S2 du grilling du 2026-07-30). Vocabulaire :
> `docs/agents/module-layout.md` (« module layout », jamais « layout » nu — ce dernier désigne le
> partitionnement layout/sémantique des nœuds dans CONTEXT.md).
> **Amende ADR-0009** : les trois couches (primitives pures, orchestration, bord) sont une taxonomie
> de *rôles*, pas de *fichiers*.
> **Ne touche pas** au découpage frontend en 8 dossiers : S3/S4, derrière un gate humain, bloqué par
> #338 et #359 — cet ADR refuse seulement de le préempter par une taxonomie de noms.

## Contexte

Le refus n'est pas « la platitude est belle ». C'est que, pour *ce* daemon, le chemin de module porte
des contrats externes, et qu'une taxonomie par nom est réfutée par le graphe d'imports réel.

## Ce qu'on refuse

### 1. Pas de sous-crates. Un seul crate `pdo-daemon`, workspace mono-membre

Scinder par couche achète des frontières de compilation que rien ne réclame : il n'existe **aucun**
second consommateur des types du daemon. La seule frontière externe réelle est
`crates/pdo-daemon/tests/`, qui consomme une poignée d'items déclarés un par un. Un split imposerait
`pub` sur tout ce qui traverse une frontière interne — l'inverse exact du resserrage que #494 vient
de faire (194 + ~90 items ramenés à `pub(crate)`).

### 2. Pas de répertoires dans `crates/pdo-daemon/src/`

Les cibles `tracing` sont `module_path!()`. Déplacer `trigger_store.rs` sous `triggers/` réécrit sa
cible en `pdo_daemon::triggers::trigger_store`, ce qui casse **en silence** tout
`RUST_LOG=pdo_daemon::trigger_store=debug` et tous les greps de runbook. Le chemin de module est ici
une **interface opérationnelle**, pas un détail de rangement.

### 3. Les trois couches d'ADR-0009 ne reçoivent pas de dossiers

« Primitive pure / orchestration / bord » est un rôle qu'un module joue, lisible dans sa signature.
Le matérialiser en dossiers obligerait à trancher le dossier de chaque module hybride, rouvrirait le
§2, et n'ajouterait rien qu'un `grep` sur les signatures ne donne déjà.

## Pourquoi c'est tenable sans dossiers

Le chemin de module **n'est pas un contrat public** : hors des trois modules consommés par `tests/`,
tout est `pub(crate)` ou `mod` privé. Ce qui garde la platitude *saine* est un couple lint + ratchet :

- `#![warn(unreachable_pub)]` + `clippy -D warnings` en CI : tout `pub` sans consommateur externe
  redevient rouge. La surface ne peut plus s'élargir par inadvertance.
- `scripts/layout-ratchet.sh` (gating dans le job `frontend`) : le nombre de fichiers directs de
  chaque dossier surveillé ne peut plus croître au-delà de sa baseline. Un nouveau fichier top-level
  est un acte délibéré, justifié en PR ; un rangement fait baisser la baseline dans le même commit.

La règle d'usage (« un concern = un fichier frère ») vit dans `docs/agents/module-layout.md` et **ne
qualifie pas** en ADR : c'est une convention révisable sans cérémonie. Seul le *refus* ci-dessus
qualifie, parce qu'il ferme des portes structurelles dont la réouverture coûte cher.

## Alternatives écartées

- **Taxonomie par préfixe de nom** (« tous les `*_store` ensemble »). Réfutée par le graphe
  d'imports : côté daemon les familles de préfixe n'ont pas de cohésion d'appels ; côté frontend la
  famille `Port*` a une intersection d'imports vide et les 16 composants `*Modal` ne partagent
  qu'**une** arête interne. Regrouper par nom rangerait des fichiers qui ne se parlent pas.
- **Un plafond fixe de fichiers** (« max 30 par dossier »). Un seuil absolu est un bikeshed et se
  traite en une fois puis dérive. Le ratchet encode la seule propriété qui compte — *ne pas grossir,
  et rétrécir quand on range* — sans nombre magique.
- **Le découpage frontend en 8 dossiers.** Pas refusé ici : reporté (gate humain, dépend de #338 et
  #359). `components/ui/` reste la seule exception de dossier, parce que `npx shadcn add` ré-aplatit
  tout le reste.

## Limites acceptées

- **La platitude reste inconfortable à parcourir à l'œil.** Assumé : le confort d'un `ls` ne vaut pas
  la casse d'une cible `tracing` ni un `pub` de fuite. Les outils portent la navigabilité.
- **Le ratchet ne juge que le *nombre*, pas la *cohésion*.** Un fichier peut être mal placé sans
  faire bouger le compte ; la revue reste le garde-fou qualitatif.
- **#494 ne range rien de la liste frontend.** Cet ADR est ce qui empêche ce report de dériver en
  « on rangera par dossiers plus tard ».

## Relations

- **ADR-0009** — amendée : les couches sont des rôles, jamais des répertoires.
- **ADR-0012(a)** — tout `git mv` massif relève d'un déplacement gated, hors périmètre S0-S2.
- **ADR-0016** — inchangée.
