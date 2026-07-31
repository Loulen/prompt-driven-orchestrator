# ADR-0035 — Un refus de complétion n'est jamais un `2xx` : le statut dit refusé, le corps dit laquelle

> Statut : accepted (grilling du 2026-07-30, issue #490). Vocabulaire : CONTEXT.md § « Contrat de refus
> de la complétion ». **Amende ADR-0023** : le `2xx` de `pdo complete` signifie toujours « ton événement
> terminal est durablement enregistré et l'avance est planifiée », mais ses *Conséquences* laissaient
> croire qu'une erreur de validation renvoyée « in-request » pouvait l'être en `2xx` — elle ne peut
> plus. **Amende ADR-0025 §3** : la convention « dire l'effet » tient pour les quatre commandes de
> boucle, mais son précédent invoqué (`mark_node_done`) était partiellement un mensonge ; cette ADR y
> ajoute la classe que 0025 n'avait pas — **noop ≠ refus**. Ne touche **ni** au détachement de la queue
> d'avance (ADR-0023), **ni** au fail-fast d'un node `script` (ADR-0017), **ni** au tombstone d'un Run
> oublié (ADR-0024, qui garde son `410`). Suit la ligne d'**ADR-0001** (sharp tool : un slug inconnu se
> rend tel quel) et d'**ADR-0004** (aucun critère fermé sans test de couche ≥ 3).

## Contexte

Le chemin par lequel **tout** node se termine — `pdo complete` côté agent, *Mark complete* côté UI —
a dix-neuf sorties. Un recensement du 2026-07-30 en a trouvé **huit** qui décrivent un **refus** et
répondent malgré tout `200`, dont **quatre après avoir déjà appendé `RunFailed`** :

| Sortie | Statut d'alors | Événements déjà appendés |
|---|---|---|
| `frontmatter_retry_pending` | `200` | `FrontmatterRetryPending` |
| `frontmatter_retry_exhausted` | `200` | `NodeFailed` + `RunFailed` |
| `script_validation_failed` | `200` | `NodeFailed` + `RunFailed` |
| `doc_violated_code_immutability` | `200` | `NodeFailed` + `RunFailed` |
| `merge_conflict` | `200` | `MergeConflictDetected` + `RunFailed` |
| `merge_resolution_failed` | `200` | `MergeResolverFailed` + `RunFailed` |
| `merge_resolver_spawned` / `merge_resolver_failed` | `200` | (branches mortes, cf. §6) |

**Le consommateur manquant est la CLI, pas l'UI.** `run_complete` ne regarde que
`resp.status().is_success()` : sur ces huit corps il imprimait `Node <id> marked complete.` et sortait
en `0`. Un agent lisait donc « livré » sur un Run que le daemon venait de tuer, puis passait à la
suite — ou, pire, enchaînait `pdo fail` et doublait un échec déjà enregistré.

L'UI, elle, n'était pas muette : elle rend depuis la **projection** poussée par WebSocket, donc elle
peignait déjà du rouge — mais **en retard**, et jamais au niveau du geste. Le préjudice y est
l'**effacement** : `handleMarkComplete` remettait sa bannière à `null` *avant* d'attendre la réponse,
et un `200`-mensonge ne posait ensuite rien. Une bannière issue d'un clic précédent disparaissait pour
ne jamais revenir. À quoi s'ajoute le refus du garde de transition, seul refus déjà correctement en
`409`, que le client relisait comme `missing_outputs` avec une liste vide : gaté sur `length > 0`,
**rien** ne s'affichait. C'est le symptôme le plus fréquent — tout clic sur un node d'un Run
`RunFailed` — et il était déjà documenté comme contournement de test dans
`frontend/e2e/failed-node.spec.ts`.

## Ce qu'on décide

### 1. L'invariant remplace l'énumération

Une tentative de complétion a **quatre** issues, et une seule variante non-succès peut porter un
`2xx` :

| Issue | Statut | Événement terminal | Exit `pdo complete` |
|---|---|---|---|
| **`Completed`** | `2xx` | appendé, avance planifiée (ADR-0023) | `0` |
| **`NoOp`** | `2xx` | **aucun** | `0` |
| **`Refused`** | **jamais `2xx`** | selon la cause (cf. `recoverable`) | `3` ou `4` |
| panne / cible inconnue | `4xx`/`5xx` | — | `1` |

`NoOp` est la frontière que cette ADR ajoute au vocabulaire d'ADR-0025 : « rien à faire » reste un
succès, « ta complétion est refusée » ne l'est plus. Les confondre — ce que faisait la variante
`CompletionAttempt::Aborted`, qui emballait indistinctement un refus, un no-op légal et une
délégation réussie — est exactement ce qui rendait l'invariant inexprimable.

L'invariant se pose donc sur une variante **`Refused`** et sur un type dédié,
`CompletionRefusal`, qui **ne porte aucun statut** : la projection vers HTTP en est la seule
propriétaire (`refusal_response`), et son énumération de statuts est fermée sur `409`/`410`/`500`.
« Un refus qui répond `2xx` » devient inexprimable, et
`a_refusal_never_projects_to_a_2xx` le prouve variante par variante derrière un `match` exhaustif —
ajouter une variante sans la couvrir **ne compile plus**.

**« Jamais `2xx` » ≠ « toujours `409` ».** Le `410` d'un Run oublié (#328 / ADR-0024), le `404` d'une
cible inconnue et le `500` d'une panne **ne bougent pas**. Les aplatir dans le `409` serait la
mauvaise lecture de l'invariant.

Conséquence assumée : sur la route `POST …/nodes/<id>/done`, ces trois-là portent maintenant le corps
JSON du §3 (`{error, recoverable, message}`) au lieu du texte brut d'avant. Leur **statut** est
inchangé, et le gain est qu'un appelant unique — la CLI — sait discriminer sans avoir à connaître deux
langages de réponse sur la même route. Le `410` y voit donc son `error` passer de la prose au slug
`run_forgotten`, la prose migrant vers `message` comme pour le garde. La route `POST …/commands` garde
son propre gate de tombstone (ADR-0024 §3 en place exactement deux frontières), dont le corps ne bouge
pas.

### 2. `409`, ni `422` ni `202`

`missing_outputs` et les deux `frontmatter_*` sont les **bras d'un même `match` sur
`ValidationError`** : même validateur, même remède, node `running` dans les trois cas — et
`missing_outputs` était **déjà** un `409`. Les séparer par statut aurait fait porter au statut une
distinction que le corps porte mieux.

`422` qualifie le **contenu de la requête**. Ici la requête est `{"iter":1}` : parfaitement traitable.
Ce qui entre en conflit est l'état du disque face au contrat de sortie déclaré — la définition même du
`409`.

**`202` pour `frontmatter_retry_pending` a été instruit et rejeté.** L'argument pour : ce n'est pas un
échec, aucun événement d'échec n'est appendé, un message correctif est déjà parti dans la session, et
l'agent *doit* retenter. Deux raisons de refuser : (i) `202` est un `2xx`, donc il rend l'invariant du
§1 inexprimable — or la complétion **n'a pas été accordée**, ce qui est la définition opérationnelle
d'un refus ; (ii) l'objection « la CLI va donc échouer dans la session qui vient de recevoir le
message correctif » est **annulée par le contrat d'exit du §4** : le code `3` dit « encore ton tour,
corrige et rappelle », ce qui est *le même message* que le nudge, en accord et non en contradiction.

### 3. Le statut dit refusé, le corps dit laquelle

Forme **unique** de tout refus, sur les deux routes (`POST …/nodes/:id/done` et `POST …/commands`) :

```json
{ "error": "<slug>", "recoverable": true|false, "…détail spécifique" }
```

- **`error`** — slug `snake_case` stable, **le** point de discrimination. Un client qui branche sur le
  statut est en faute : un statut n'a pas assez de bits pour neuf causes, et la branche qui relisait
  *tout* `409` comme `missing_outputs` est la démonstration du mode d'échec.
- **`recoverable`** — répond à une seule question : *est-ce encore ton tour ?* `true` ⇒ le node est
  toujours `running`, **rien de terminal n'a été enregistré**, corrige et rappelle. `false` ⇒ le daemon
  a **déjà** enregistré l'issue terminale ; **ne jamais** enchaîner sur `pdo fail`.
- **Le détail reste verbatim celui d'avant** (`missing`, `violations`, `detail`, `reason`) : cette
  décision déplace un statut et ajoute deux clés, elle ne renomme aucun champ.
- Un slug **inconnu d'un client** se rend tel quel, jamais masqué (ADR-0001).

La convention est **transversale**, pas locale à ce chemin : c'est la forme à reprendre pour tout refus
futur. Corollaire immédiat : le refus du garde de transition (#212/#354) rejoint le contrat de corps —
son statut ne change pas (il était déjà `409`), mais sa prose passe de `error` à `message`, et `error`
devient le slug `completion_rejected`. `apiErrorMessage` côté frontend lit déjà
`body.message ?? body.error ?? fallback`, donc la prose reste lisible sans code neuf.

### 4. Le contrat d'exit de `pdo complete` : `0` / `3` / `4` / `1`

| Situation | Code | Ce que l'appelant doit faire |
|---|---|---|
| succès, ou **doublon légal** (`noop`) | `0` | rien |
| refus **récupérable** (`recoverable: true`) | `3` | corriger, rappeler `pdo complete`. **Pas** `pdo fail` |
| refus **terminal** (`recoverable: false`) | `4` | s'arrêter et rapporter. **Pas** `pdo fail` — c'est déjà enregistré |
| panne, transport, corps illisible, `5xx` | `1` | ici seulement, `pdo fail` est le bon conseil |

Ces codes sont un contrat **public** : ils vivent dans le bash d'auteurs de pipelines, et
`pdo complete` est la seule sous-commande à en porter un.

**Le `4` existe pour un `||` situé six mille lignes plus loin.** Le tail bash d'un node `script` fait
`pdo complete || pdo fail --reason "…"`. Ce `||` était **du code mort** tant que son déclencheur
répondait `200` — le §1 le réveille. Sans discrimination du `4`, chaque refus terminal d'un node
`script` produirait **deux** `NodeFailed` et **deux** `RunFailed`, le second avec une raison fausse
(`NodeFailed` est absorbé par le garde, `RunFailed` ne l'est pas). Le code `4` n'a donc de valeur que
si **le tail le teste** : les deux changements sont indissociables, et
`build_script_tail_does_not_double_fail_on_a_refused_completion` les épingle ensemble.

**Le `0` sur un doublon légal est non négociable** : le no-op est documenté « *legal duplicate […] do
not surface an error* » dans le garde de transition. Un agent perplexe qui rappelle `pdo complete` ne
doit pas lire « refusé » puis enchaîner `pdo fail` — il tuerait un Run qui vient de réussir. Et sur un
node `script`, le `||` du tail le ferait **sans demander**.

### 5. Le détail imbriqué se répare chez le consommateur

Le fail-fast d'un node `script` imbrique sa preuve sous `payload.detail.{kind, violations|missing}` là
où le chemin après-retry la met à plat sous `payload.violations`. Le projecteur ne lisait que la forme
plate : le détail déjà calculé était **jeté**, et l'UI peignait une bannière rouge avec une liste vide.

C'est **le projecteur** qui apprend à lire les deux formes, pas le producteur qui aplatit. Trois
raisons :

1. aplatir rendrait la trace d'audit d'un fail-fast `script` **indistinguable** d'un échec après
   retry — deux causes, deux remèdes, une seule empreinte ;
2. cela routerait le cas vers une bannière titrée « after retry », alors qu'un node `script` ne
   retente **jamais** ;
3. cela ne répare que **la moitié** du bug : `ValidationError::MissingOutputs` n'a aucun `violations`
   à aplatir, seulement un `missing` — qui, avant #490, n'avait **aucun** foyer, ni Rust ni TS.

Corollaire : `NodeState` gagne un `missing_outputs`, `#[serde(default, skip_serializing_if)]` comme
son voisin `frontmatter_violations`, donc **absent** de toute réponse existante — la compatibilité
filaire est identique à l'octet pour tout échec non-`script`. Et le titre de la bannière rouge devient
la `failure_reason` **verbatim** au lieu d'une chaîne codée en dur.

### 6. Deux bras morts passent sous le type, sans être supprimés

`merge_resolver_spawned` et `merge_resolver_failed` sont **inatteignables** : ils supposent
`MergeResult::ConflictPendingResolution`, que seul `keep_conflict == true` construit, et aucun appelant
de production ne passe `true` (ADR-0006 a retiré le résolveur automatique). Ils suivent les rails du
nouveau type à coût de test nul.

Leur **suppression** emporterait tout un sous-système vestigial (`MergeResolverStarted/Failed/
Completed`, `RunState.merge_resolver`, `MergeResolverInfo` côté TS, `prompts/builtin/merge-resolver.md`,
la route `__merge_resolver__`) : c'est une autre issue, à ouvrir en retombée d'ADR-0006. Supprimer un
sous-système sous un fix de bug mélangerait deux intentions sous un seul bump.

En revanche la **délégation** au résolveur est hissée : le `if node_id == MERGE_RESOLVER_NODE_ID` quitte
le corps partagé pour le handler HTTP. Trois bénéfices : la branche emballait un **succès complet**
(elle a appendé `NodeCompleted` et appelé `complete_node`) dans la variante d'échec, donc le type
devient `Completed | NoOp | Refused` **sans trou** ; l'invariant devient **total** ; et le contournement
du garde de transition que cette branche opérait — elle était placée *avant* le garde — disparaît. Le
hissage est sûr parce que l'autre appelant du corps partagé, la veille de vivacité, ne peut **jamais**
voir `__merge_resolver__` : aucune session de résolveur n'est jamais spawnée.

## Alternatives écartées

- **Corriger le frontend seul** (option 1 du ticket). Le consommateur lésé est la **CLI** : un agent qui
  sort en `0` sur un Run mort ne voit aucune UI. Et le mensonge resterait sur le wire pour tout autre
  client (`curl`, harnais de test, scripts).
- **Garder le `200` en enrichissant le corps.** C'est l'approche de #489, légitime **là-bas** parce
  qu'additive sur une surface qui ne mentait pas. Ici le statut *est* le mensonge : un client bien
  écrit qui teste `resp.ok` a raison de conclure « accordé ».
  *(faux — corrigé par #489/ADR-0037 : la surface mentait déjà, et de la même façon.
  `restart_node` répondait `200` sur un `node_id` absent du pipeline — le `find` du bras étant
  **après** le kill de la session et après l'append du `CommandIssued`, en violation d'ADR-0025 §2 —
  et `200` sur les cinq `SpawnOutcome`, y compris `Failed`, dont trois des quatre producteurs
  appendent `RunFailed` : exactement le motif recensé ci-dessus, sur une autre route. Son unique
  `409` portait de la prose dans `error`, sans `recoverable`. #489 n'est donc pas additif : il
  déplace deux statuts **et** réécrit un corps.)*
- **Énumérer les huit bras** et poser une assertion sur chacun. Le recensement a montré que la liste
  bouge (deux bras morts, un succès emballé en échec, un bras qui court-circuitait le point de
  passage) : une liste à relire est un invariant qui dérive. Le type, lui, ne dérive pas.
- **Poser le garde sur `CompletionAttempt`** (un `debug_assert` dans son constructeur d'échec). Rejeté
  deux fois : il ne voit pas le bras `mark_node_done`, qui n'a jamais construit de `CompletionAttempt`
  — c'est-à-dire tout le chemin de l'UI ; et il **tirerait sur du comportement correct** (le no-op, la
  délégation). Le vrai point de passage partagé est le validateur de sortie.
- **`422` ou `202`** — cf. §2.
- **Un seul code de sortie non nul.** « Refusé » et « refusé, et le Run est mort » demandent deux
  gestes opposés du bash appelant. Un code unique laisserait le tail `script` doubler l'échec.

## Limites acceptées

- **Les corps de succès restent asymétriques** : `POST …/done` répond `200 "ok"` en **texte brut**,
  `POST …/commands` répond `200 {"ok":true}`. Symétriser appartient à **#491** ; le faire ici
  mélangerait une rupture et un nettoyage sous un seul bump.
- **`vitest` ne tourne pas en CI** (le job frontend fait `npm ci` / `typecheck` / `lint` / `build`). La
  branche client généralisée — le seul endroit où un refus peut *encore* échouer en silence — atterrit
  donc dans la seule couche qu'aucun gate ne joue. D'où l'assertion Playwright obligatoire, et
  `make test` déclaré à la main dans la PR.
- **Le bouton *Mark complete* reste gaté sur le statut seul** (`NodeState` ne porte pas `node_type`),
  donc il s'affiche aussi sur un node `script` `failed`. Le masquer retirerait un **chemin de
  récupération supporté** — le garde de transition autorise explicitement « mark_node_done on a failed
  node (outputs fixed by hand) ». Le clic peut désormais être refusé **visiblement**, ce qui est le
  bon comportement.
- **Deux bras morts restent sous le type** (§6).
- **La branche fail-fast `script` n'est pas idempotente** : un clic refusé sur un node `script` déjà
  `failed` réentre dedans et appende un **second** `NodeFailed` + `RunFailed` *avant* de répondre — un
  effet de bord sur un refus. Constaté ici, à ficher séparément : ce n'est pas un statut menteur, c'est
  une idempotence manquante.
- **L'asymétrie de route subsiste** : `merge_conflict` et `doc_violated_code_immutability` sont
  atteignables depuis `POST …/done` (donc depuis chaque `pdo complete`) et **pas** depuis
  `POST …/commands`, dont le bras ne fait ni le merge ni le contrôle d'immutabilité. Cette ADR ne la
  crée pas et ne la ferme pas ; elle la nomme.

## Relations

- Issue **#490** (cette décision). **#491** vient **après** (symétrie des corps de succès, filet de
  content-types). **#489** porte la même faute sur une autre route : il n'est ni séparé de
  l'invariant, ni additif (voir l'errata des Alternatives écartées). Il est livré par
  **ADR-0037**, qui reprend telle quelle la forme de corps posée ici.
- **ADR-0025** (#327) — amendée : la convention noop tient, son précédent était partiellement faux,
  son vocabulaire gagne **noop ≠ refus**.
- **ADR-0023** (#304) — amendée : « `2xx` = enregistré + planifié » devient aussi « `2xx` = pas
  refusé ». Le détachement de la queue est intact.
- **ADR-0017** (#248) — le fail-fast d'un node `script` est intact ; seul son statut change, et son
  détail imbriqué est enfin lu.
- **ADR-0032** (#469) — indemne par construction : la veille lit la **variante**, jamais le statut.
- **ADR-0024** (#328) — le `410` du Run oublié est conservé tel quel.
- **ADR-0006** — retombée à ficher : suppression du résolveur de merge automatique (§6).
- **ADR-0001** — un slug inconnu se rend tel quel.
- **ADR-0004** — les critères sont fermés par des tests de couche ≥ 3 : invariant structurel, tests
  filaires sur les **deux** routes, tests de code de sortie sur le vrai binaire, assertion Playwright.
- **ADR-0009** — le refus est rendu à la couche 3 (l'UI), au niveau du geste.
- **#212 / #354** — le garde de transition, dont le refus rejoint le contrat de corps.
