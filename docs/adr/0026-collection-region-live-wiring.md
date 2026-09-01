# ADR-0026 — Câblage live des régions `collection` et retrait de ForEach

Date : 2026-07-12 · Statut : accepté · Issue : #269 (Direction C, ratifiée 2026-07-03)

## Contexte

Sans cette ADR, une région `kind: collection` écrite à la main **no-ope silencieusement** : ADR-0011
a remplacé `ForEach` par la région, #151 a livré le rendu et le nudge, mais ni le geste ni le
câblage runtime — le scheduler live pilotait toujours `NodeType::ForEach` et le moteur pur de
`loop_region.rs` était du code mort. Footgun contraire à « jamais de stall silencieux », et une
bannière de canvas qui promet un contrôle inexistant.

## Décision

Compléter la verticale en un seul mouvement :

1. **Moteur live** — le scheduler reconnaît une edge externe qui **entre** dans une région
   `collection` et délègue au moteur : fan-out de l'entry, un lap par item, items déposés dans les
   artefacts de l'itération (l'entry lit **son propre** item — pas de nœud driver). Les edges
   membre→non-membre sont des **sorties de barrière** : supprimées au fil des laps, tirées une seule
   fois quand tous les laps sont complétés. Collection vide ⇒ barrière immédiate. Projection keyée
   par **id de région** (miroir de `foreach_states`).
   - `ready_nodes` ne spawne jamais un membre nourri par un producteur externe (le fan-out s'en
     charge) ; une cible de barrière n'est prête que région `done` — le statut nœud reflète le
     *dernier* événement, donc un membre peut projeter `Completed` en plein fan-out.
   - Un Run ne se termine jamais au milieu d'un fan-out : la complétion exige toutes les régions
     collection `done`.
2. **Geste canvas** — menu contextuel sur le(s) membre(s) sélectionné(s) : « Fan out over "<field>" »
   (un champ frontmatter `type: list` porté par une edge entrante externe). Le sérialiseur FE émet
   désormais `over:` (il le droppait).
3. **`pdo migrate`** — la dissolution ForEach→région reçoit son premier point d'entrée prod :
   one-shot **explicite** (jamais d'auto-heal au load), `--dry-run`, backup `.bak`. Garde
   anti-collision : l'id de région (= le `name` du ForEach, texte libre) est dédupliqué contre les
   ids de nœuds, les régions existantes et les ForEach homonymes, en ordre document (déterministe).
4. **Retrait de `NodeType::ForEach`** — `type: for-each`/`foreach` est **refusé au parse** avec un
   message qui nomme `pdo migrate` (jamais warn+coerce : réécrire un fan-out en doc-only exécuterait
   zéro fois le travail par item). Les décodeurs d'événements ForEach restent en **legacy
   read-only** pour les runs archivés.

## Périmètre v1

**Bodies mono-membre uniquement** : un nœud fan-out par item → barrière → aval. Les bodies
multi-nœuds / imbriqués exigent un keying par-itération de `run_state.nodes` non résolu —
**différé** derrière un nouvel ADR. Le geste accepte une multi-sélection (le modèle YAML la porte),
mais le moteur ne barrière correctement que le cas mono-membre sanctionné par ADR-0011.

## Conséquences

- Deux moteurs de fan-out ne coexistent pas sur `main` : ForEach est retiré dans le même mouvement.
- Les YAML legacy `for-each` ne parsent plus tant que `pdo migrate` n'a pas tourné — refus
  **bruyant et actionnable**, choisi contre la coercition silencieuse.
