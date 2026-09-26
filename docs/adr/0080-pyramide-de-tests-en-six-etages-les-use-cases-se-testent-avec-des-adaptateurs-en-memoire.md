# Pyramide de tests en six étages : les use cases se testent avec des adaptateurs en mémoire

> Statut : accepted (grilling du 2026-09-26, story « architecture extensible »).
> **Remplace ADR-0004.** Le principe qui en reste : **une AC ne se ferme pas sans un test contre le
> vrai système** (étage 4 ou plus). Ce qui change : l'hexagonal (ADR-0079) crée un étage de use
> cases testés avec des faux ports, et les tests `oneshot` du router, qui faisaient doublon avec
> le vrai daemon, disparaissent.

## Décision

1. **Domaine.** Tests unitaires des règles pures : graphe, transitions, coût, cron. Aucune I/O.
2. **Use cases.** La couche application est testée avec des **adaptateurs en mémoire** injectés
   dans ses ports. C'est là que se teste la logique d'orchestration, rapidement et sans tmux.
   ADR-0004 interdisait tout mock au-dessus du domaine. On lève cette interdiction pour cet étage
   seulement, parce que l'étage 4 continue de prouver le vrai câblage.
3. **Adaptateurs.** Chaque adaptateur est testé contre la vraie ressource : SQLite, git sur un
   dépôt temporaire, tmux, docker.
4. **HTTP.** Le vrai daemon sur un port éphémère, avec reqwest. **Au moins un test par route**, et
   un test vérifie cette règle contre le router assemblé.
5. **E2E.** Quelques parcours dans le vrai navigateur (Playwright).
6. **Agentique.** Une FP par ticket (gate de l'auto-merge dans l'intégration), au plus 3 HP.
   `/agentic-tests`, hors CI.

La suite contient aussi deux **tests de structure** : le test d'architecture (le domaine d'un
contexte n'importe rien de technique) et le plafond de 400 lignes par fichier de production, avec
une baseline qui ne peut que baisser (`docs/agents/module-layout.md`).

**La résilience n'est pas un Happy Path.** Les invariants d'adversité (mort de session, kill du
daemon, fuite de slot d'admission, jamais de stall silencieux) restent couverts en permanence à
l'étage 4.

## Alternatives écartées

- **Garder les tests `oneshot`** (router en mémoire avec un faux état global). Ils testaient le
  même contrat que l'étage 4, avec du câblage manuel en plus, et ils cassent dès que l'état global
  disparaît.
- **Une suite permanente de snapshots de réponses complètes.** Elle ferait doublon avec l'étage 4
  et les types générés. Seule une suite **temporaire** de ce genre sert de filet pendant la
  migration, et elle est supprimée à la fin.
