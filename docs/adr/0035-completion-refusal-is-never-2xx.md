# ADR-0035 — Un refus de complétion n'est jamais un `2xx` : le statut dit refusé, le corps dit laquelle

> Statut : accepted (grilling du 2026-07-30, issue #490). Vocabulaire : CONTEXT.md § « Contrat de
> refus de la complétion ». **Amende ADR-0023** : le `2xx` de `pdo complete` signifie toujours
> « ton événement terminal est durablement enregistré et l'avance est planifiée », mais ses
> *Conséquences* laissaient croire qu'une erreur de validation renvoyée « in-request » pouvait
> l'être en `2xx` — elle ne peut plus. **Amende ADR-0025 §3** : la convention « dire l'effet »
> tient pour les quatre commandes de boucle, mais son précédent invoqué (le chemin de complétion
> UI) était partiellement un mensonge ; cette ADR y ajoute la classe que 0025 n'avait pas —
> **noop ≠ refus**. Ne touche ni au détachement de la queue d'avance (ADR-0023), ni au fail-fast
> d'un node `script` (ADR-0017), ni au tombstone d'un Run oublié (ADR-0024, qui garde son `410`).
> Suit ADR-0001 (un slug inconnu se rend tel quel) et ADR-0004 (aucun critère fermé sans test de
> couche ≥ 3).

## Contexte

Le chemin par lequel **tout** node se termine — `pdo complete` côté agent, *Mark complete* côté
UI — a dix-neuf sorties. Un recensement du 2026-07-30 en a trouvé **huit** qui décrivent un
**refus** et répondaient malgré tout `200`, dont **quatre après avoir déjà appendé `RunFailed`**
(échec de validation de script, violation d'immutabilité doc-only, conflit de merge, retries de
frontmatter épuisés — plus le retry pendant, et deux branches mortes du résolveur de merge).

**Le consommateur manquant est la CLI, pas l'UI.** `pdo complete` ne regardait que le succès HTTP :
sur ces huit corps il imprimait « marked complete » et sortait en `0`. Un agent lisait donc
« livré » sur un Run que le daemon venait de tuer, puis passait à la suite — ou, pire, enchaînait
`pdo fail` et doublait un échec déjà enregistré.

L'UI, elle, n'était pas muette (elle rend depuis la projection WebSocket, donc elle peignait déjà
du rouge) mais **en retard, et jamais au niveau du geste** : un `200`-mensonge effaçait la bannière
du clic sans jamais la remplacer, et le seul refus déjà en `409` — le garde de transition — était
relu par le client comme une liste vide, donc rien ne s'affichait. C'est le symptôme le plus
fréquent (tout clic sur un node d'un Run failed), déjà documenté comme contournement dans les tests
e2e.

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
fourre-tout d'avant, qui emballait indistinctement un refus, un no-op légal et une délégation
réussie — est exactement ce qui rendait l'invariant inexprimable.

L'invariant est posé comme **propriété d'un type**, pas comme liste de bras à relire : le type de
refus ne porte **aucun** statut, sa projection vers HTTP en est la seule propriétaire et son
énumération de statuts est fermée sur `409`/`410`/`500` — « un refus qui répond `2xx` » devient
inexprimable, et ajouter une variante sans la couvrir ne compile plus. La délégation au résolveur
de merge — un succès complet emballé jusque-là dans la variante d'échec, placé *avant* le garde de
transition qu'il court-circuitait — est hissée hors du corps partagé, ce qui rend le type sans trou
et l'invariant **total**.

**« Jamais `2xx` » ≠ « toujours `409` ».** Le `410` d'un Run oublié (ADR-0024), le `404` d'une
cible inconnue et le `500` d'une panne **ne bougent pas** ; les aplatir dans le `409` serait la
mauvaise lecture de l'invariant. Conséquence assumée : sur la route de complétion, ces trois-là
portent maintenant le corps JSON du §3 au lieu du texte brut — statut inchangé, et le gain est
qu'un appelant unique (la CLI) discrimine sans connaître deux langages de réponse sur la même
route. Le `410` y voit son `error` passer de la prose au slug `run_forgotten`. La route de
commandes garde son propre gate de tombstone (ADR-0024), dont le corps ne bouge pas.

### 2. `409`, ni `422` ni `202`

Les refus de validation de sortie sont les bras d'un même match — même validateur, même remède,
node `running` dans les trois cas — et le manque d'outputs était **déjà** un `409`. Les séparer par
statut aurait fait porter au statut une distinction que le corps porte mieux.

`422` qualifie le **contenu de la requête**. Ici la requête est parfaitement traitable ; ce qui
entre en conflit est l'état du disque face au contrat de sortie déclaré — la définition même du
`409`.

**`202` pour le retry de frontmatter pendant a été instruit et rejeté.** L'argument pour : ce n'est
pas un échec, aucun événement d'échec n'est appendé, un message correctif est déjà parti dans la
session, et l'agent *doit* retenter. Deux raisons de refuser : (i) `202` est un `2xx`, donc il rend
l'invariant du §1 inexprimable — or la complétion **n'a pas été accordée**, ce qui est la
définition opérationnelle d'un refus ; (ii) l'objection « la CLI va donc échouer dans la session
qui vient de recevoir le message correctif » est **annulée par le contrat d'exit du §4** : le code
`3` dit « encore ton tour, corrige et rappelle », *le même message* que le nudge, en accord et non
en contradiction.

### 3. Le statut dit refusé, le corps dit laquelle

Forme **unique** de tout refus, sur les deux routes (complétion et commandes) :

```json
{ "error": "<slug>", "recoverable": true|false, "…détail spécifique" }
```

- **`error`** — slug `snake_case` stable, **le** point de discrimination. Un client qui branche sur
  le statut est en faute : un statut n'a pas assez de bits pour neuf causes, et la branche qui
  relisait *tout* `409` comme un manque d'outputs est la démonstration du mode d'échec.
- **`recoverable`** — répond à une seule question : *est-ce encore ton tour ?* `true` ⇒ le node est
  toujours `running`, rien de terminal n'a été enregistré, corrige et rappelle. `false` ⇒ le daemon
  a **déjà** enregistré l'issue terminale ; **ne jamais** enchaîner sur `pdo fail`.
- **Le détail reste verbatim celui d'avant** : cette décision déplace un statut et ajoute deux
  clés, elle ne renomme aucun champ.
- Un slug **inconnu d'un client** se rend tel quel, jamais masqué (ADR-0001).

La convention est **transversale**, pas locale à ce chemin : c'est la forme à reprendre pour tout
refus futur. Corollaire immédiat : le refus du garde de transition (#212/#354) rejoint le contrat
de corps — statut inchangé (déjà `409`), sa prose passe dans `message` et `error` devient le slug
`completion_rejected`.

### 4. Le contrat d'exit de `pdo complete` : `0` / `3` / `4` / `1`

| Situation | Code | Ce que l'appelant doit faire |
|---|---|---|
| succès, ou **doublon légal** (`noop`) | `0` | rien |
| refus **récupérable** (`recoverable: true`) | `3` | corriger, rappeler `pdo complete`. **Pas** `pdo fail` |
| refus **terminal** (`recoverable: false`) | `4` | s'arrêter et rapporter. **Pas** `pdo fail` — c'est déjà enregistré |
| panne, transport, corps illisible, `5xx` | `1` | ici seulement, `pdo fail` est le bon conseil |

Ces codes sont un contrat **public** : ils vivent dans le bash d'auteurs de pipelines, et
`pdo complete` est la seule sous-commande à en porter un.

**Le `4` existe pour un `||` situé loin de lui.** Le tail bash d'un node `script` fait
`pdo complete || pdo fail --reason "…"`. Ce `||` était du code mort tant que son déclencheur
répondait `200` — le §1 le réveille. Sans discrimination du `4`, chaque refus terminal d'un node
`script` produirait **deux** `NodeFailed` et **deux** `RunFailed`, le second avec une raison
fausse. Le code `4` n'a donc de valeur que si le tail le teste : les deux changements sont
indissociables.

**Le `0` sur un doublon légal est non négociable** : le no-op est documenté « legal duplicate » par
le garde de transition. Un agent perplexe qui rappelle `pdo complete` ne doit pas lire « refusé »
puis enchaîner `pdo fail` — il tuerait un Run qui vient de réussir. Et sur un node `script`, le
`||` du tail le ferait sans demander.

## Alternatives écartées

- **Corriger le frontend seul** (option 1 du ticket). Le consommateur lésé est la **CLI** : un
  agent qui sort en `0` sur un Run mort ne voit aucune UI. Et le mensonge resterait sur le wire
  pour tout autre client.
- **Garder le `200` en enrichissant le corps.** Écarté : le statut *est* le mensonge — un client
  bien écrit qui teste le succès HTTP a raison de conclure « accordé ». (#489, d'abord cru
  « séparé et additif », portait en réalité **la même faute sur une autre route** — `restart_node`
  répondait `200` sur toutes les issues d'un spawn, y compris l'échec, après avoir tué la session.
  Il est livré par **ADR-0037**, qui reprend telle quelle la forme de corps posée ici.)
- **Énumérer les huit bras** et poser une assertion sur chacun. Le recensement a montré que la
  liste bouge (deux bras morts, un succès emballé en échec, un bras qui court-circuitait le point
  de passage) : une liste à relire est un invariant qui dérive. Le type, lui, ne dérive pas.
- **Poser le garde sur le type de tentative** (assertion dans son constructeur d'échec). Rejeté
  deux fois : il ne voit pas le bras UI, qui ne construisait jamais ce type ; et il tirerait sur du
  comportement correct (le no-op, la délégation). Le vrai point de passage partagé est le
  validateur de sortie.
- **`422` ou `202`** — cf. §2.
- **Un seul code de sortie non nul.** « Refusé » et « refusé, et le Run est mort » demandent deux
  gestes opposés du bash appelant. Un code unique laisserait le tail `script` doubler l'échec.

## Limites acceptées

- **Les corps de succès restent asymétriques** entre les deux routes (texte brut vs JSON).
  Symétriser appartient à **#491** ; le faire ici mélangerait une rupture et un nettoyage sous un
  seul bump.
- **Le bouton *Mark complete* reste gaté sur le statut seul**, donc il s'affiche aussi sur un node
  `script` `failed`. Le masquer retirerait un chemin de récupération supporté (le garde de
  transition autorise explicitement la complétion d'un node failed dont les outputs ont été réparés
  à la main). Le clic peut désormais être refusé **visiblement**, ce qui est le bon comportement.
- **La branche fail-fast `script` n'est pas idempotente** : un clic refusé sur un node `script`
  déjà `failed` réentre dedans et double l'échec avant de répondre — un effet de bord sur un refus.
  Constaté ici, à ficher séparément : ce n'est pas un statut menteur, c'est une idempotence
  manquante.
- **L'asymétrie de route subsiste** : le conflit de merge et la violation d'immutabilité sont
  atteignables depuis la route de complétion (donc depuis chaque `pdo complete`) et pas depuis la
  route de commandes, dont le bras ne fait ni le merge ni le contrôle. Cette ADR ne la crée pas et
  ne la ferme pas ; elle la nomme.

## Relations

- Issue **#490** (cette décision). **#491** vient après (symétrie des corps de succès). **#489**
  porte la même faute sur une autre route et est livré par **ADR-0037**, qui reprend la forme de
  corps posée ici.
- **ADR-0025** (#327) — amendée : la convention noop tient, son précédent était partiellement faux,
  son vocabulaire gagne **noop ≠ refus**.
- **ADR-0023** (#304) — amendée : « `2xx` = enregistré + planifié » devient aussi « `2xx` = pas
  refusé ». Le détachement de la queue est intact.
- **ADR-0017** (#248) — le fail-fast d'un node `script` est intact ; seul son statut change.
- **ADR-0032** (#469) — indemne par construction : la veille lit la **variante**, jamais le statut.
- **ADR-0024** (#328) — le `410` du Run oublié est conservé tel quel.
- **ADR-0006** — retombée à ficher : la suppression du sous-système vestigial du résolveur de merge
  (bras inatteignables depuis le retrait du résolveur automatique) est une autre issue ; la mêler
  ici aurait mélangé deux intentions sous un seul bump.
- **ADR-0001** — un slug inconnu se rend tel quel. **ADR-0004** — critères fermés par des tests de
  couche ≥ 3. **#212 / #354** — le garde de transition, dont le refus rejoint le contrat de corps.
