# ADR-0026 — Câblage live des régions `collection` et retrait de ForEach

Date : 2026-07-12 · Statut : accepté · Issue : #269 (Direction C, ratifiée 2026-07-03)

## Contexte

ADR-0011 a remplacé le nœud `ForEach` par la région `kind: collection` (driver
`over: <field>`, naissance par geste explicite). #151 a livré le rendu et le
nudge, mais **ni le geste ni le câblage runtime** : le scheduler live pilotait
toujours `NodeType::ForEach`, et le moteur pur de `loop_region.rs`
(`resolve_collection` / `collection_fanout` / `collection_barrier_*`) était du
code mort. Une région `collection` écrite à la main **no-opait silencieusement**
(footgun contraire à « jamais de stall silencieux »), et la bannière du canvas
promettait un contrôle inexistant.

## Décision

Compléter la verticale en un seul mouvement (ordre 1→4→2→3, D1) :

1. **Moteur live** — le scheduler reconnaît une edge externe qui **entre** dans
   une région `collection` et délègue au moteur : fan-out de l'entry, un lap
   par item (`Spawn iter 1..=total`), items déposés en
   `<artifacts>/<entry>/iter-N/_item.md` (l'entry lit **son propre** item — pas
   de nœud driver). Les edges membre→non-membre sont des **sorties de
   barrière** : supprimées au fil des laps, tirées une seule fois quand tous
   les laps sont complétés (`CollectionDone`). Collection vide ⇒ barrière
   immédiate. Projection : événements `collection_{started,empty,done}` →
   `RunState.collection_states` keyé par **id de région** (miroir de
   `foreach_states`).
   - `ready_nodes` ne spawne jamais un membre nourri par un producteur externe
     (le fan-out s'en charge) ; une cible de barrière n'est prête que région
     `done` (le statut nœud reflète le *dernier* événement, donc un membre peut
     projeter `Completed` en plein fan-out).
   - `should_complete_run` exige toutes les régions collection `done` — un
     Run ne se termine jamais au milieu d'un fan-out.
2. **Geste canvas** — menu contextuel sur le(s) membre(s) sélectionné(s) :
   « Fan out over "<field>" » (un item par champ frontmatter `type: list` porté
   par une edge entrante externe) → `createCollectionRegion(members, over)`.
   Le sérialiseur FE émet désormais `over:` (il le droppait).
3. **`pdo migrate`** — la dissolution ForEach→région (migrateur) reçoit son
   premier point d'entrée prod : one-shot **explicite** (jamais d'auto-heal au
   load), `--dry-run`, backup `.bak` par fichier migré. Garde anti-collision :
   l'id de région (= le `name` du ForEach, texte libre) est dédupliqué contre
   les ids de nœuds, les régions existantes et les ForEach homonymes
   (suffixe `-2`), en ordre document (déterministe).
4. **Retrait de `NodeType::ForEach`** — la variante disparaît ; `type:
   for-each`/`foreach` est **refusé au parse** avec un message qui nomme
   `pdo migrate` (jamais warn+coerce : réécrire un fan-out en doc-only
   exécuterait zéro fois le travail par item). `EventKind::ForEach*` et
   `apply_foreach_event` restent en **décodeurs legacy read-only** pour les
   runs archivés.

## Périmètre v1 (D2)

**Bodies mono-membre uniquement** : un nœud fan-out par item → barrière →
aval. Les bodies multi-nœuds / imbriqués (itération composite) exigent un
keying par-itération de `run_state.nodes` non résolu — **différé** derrière un
nouvel ADR, hors #269. Le geste accepte une multi-sélection (le modèle YAML la
porte), mais le moteur ne barrière correctement que le cas mono-membre
sanctionné par ADR-0011 (« itération plate »).

## Conséquences

- La bannière nudge redevient honnête : le contrôle qu'elle nomme existe.
- Deux moteurs de fan-out ne coexistent pas sur `main` : ForEach est retiré
  dans le même mouvement que le câblage collection.
- Les YAML legacy `for-each` sur disque ne parsent plus tant que `pdo migrate`
  n'a pas tourné — refus **bruyant et actionnable**, choisi contre la
  coercition silencieuse.
