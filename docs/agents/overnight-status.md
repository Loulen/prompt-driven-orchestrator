# Briefing nuit — 2026-06-05 → matin

Mission autonome pendant ton sommeil. Ce fichier est gitignoré et tenu à jour à chaque étape.

## TL;DR (lis-moi en premier)
- Tout le travail vit sur des **branches d'intégration** (`wf/.../integration`). **Rien n'est mergé sur ta branche** (`docs/triggers-design`) — le gate humain est à toi. Ton checkout : intact (`docs/triggers-design`, seul `main.tsx` WIP).
- **BRANCHE À RELIRE EN PRIORITÉ : `wf/1780697052-25210/integration` @ `55e467d`** = main + ADR-0011 + #146 + #144 + correctif Merge (da0d72e) + nouvelle maquette (2eaa4ed) + **#147, #145, #149, #153, #154** (tous L5 PASS). C'est le livrable canvas non-boucle, complet et vert.
- #148 (bounded loop) tourne, empilé dessus. Triggers prévus après. Boucle #150/151/152 : en attente de ta revue de #148.

## Ce qui est FAIT et vert (à relire/merger)
Sur `wf/1780697052-25210/integration` (chaque issue a son scénario L5, tous PASS) :
- #146 remove DagCanvas · #144 conditional edges + **correctif convergence Merge** (edge-résolution, halt explicite, addendum ADR-0006) · #147 edge detail panel (+ **SwitchNode retiré**) · #145 start node images · #149 emergent ports + slim cards · #153 inspector pooled inputs · #154 orthogonal edge routing + waypoints.
- Maquette `docs/design/` rafraîchie (canvas + écrans triggers).
- **#148 bounded loop region** (sur `wf/1780712650-23824/integration` @ `67745eae`, empilé sur le canvas) : modèle `loops:` (id/members/kind:bounded/max_iter), région rendue + compteur + pills, plus de node Loop/Switch. Daemon lib 763/763 ; 15 tests layer-3 boucle (early-exit, exhausted-unrouted, coalescing). ⚠️ Runtime live des laps NON piloté en UI (symlink `pdo` cassé, voir flag) mais couvert par les tests.

## ⚠️ Flag environnement (à corriger, gêne les runs réels)
`~/.local/bin/pdo` (symlink, daté 7 mai) pointe vers `target/release/pdo` qui **n'existe pas** (release jamais buildé). Conséquence : dans chaque NodeRun spawné, le `pdo complete` de claude échoue en « command not found » → les runs **ne peuvent pas se compléter seuls** sur cette machine. Tous les testeurs L5 ont contourné via le binaire debug en chemin absolu. Fix : `cargo build --release -p pdo` OU repointer le symlink vers `target/debug/pdo`. (Aussi : des milliers de sockets tmux vides `pdo-<pid>` traînent dans `/tmp/tmux-1000/` — cruft inoffensif.)

## Plan
1. [EN COURS] Correctif du bug de convergence Merge (#144) — subagent TDD sur la branche d'intégration. Modèle edge-résolution (firée/morte) décidé ensemble.
2. [À VENIR] Subagent de TEST indépendant → valide #144 bout-en-bout (L5) ; boucle corrective bornée si besoin.
2bis. [DESIGN] La maquette a été mise à jour (nouveau lien `…/h/yzzFYtYOq9GOm3JU8dEKhw`, un .tar.gz `pdo/` = structure de `docs/design/`). 
   - [FAIT] Les 10 issues canvas (#144,145,147-154) repointées du lien vers `docs/design/project/` (vérifié : ancien token absent partout). Inspecteur (#147/150/153) → + `Inspector Component Exploration.html` ; cartes (#149) → + `Node Component Exploration.html`/`Node Variants.html`.
   - [À VENIR, après merge-fix] Remplacer le contenu de `docs/design/` par le nouveau bundle + commit sur la branche d'intégration — AVANT le batch canvas, pour que les implémenteurs codent la bonne maquette. (Fait après merge-fix pour ne pas clasher le worktree d'intégration.)
3. [À VENIR] Batch canvas NON-BOUCLE via sandcastle, EMPILÉ sur la branche d'intégration : #147 (edge authoring + retrait SwitchNode), #145, #149, #153, #154.
4. [À VENIR] #148 (fondation boucle bornée), empilé. GATE : si son L5 échoue et que le fix-loop ne résout pas → STOP, je n'empile PAS #150/#151/#152, je te flague.
5. [À VENIR / CONDITIONNEL] Triggers #158-#162 en 2e workflow scopé, après le canvas, si tout est propre.

## Correction d'une bourde de ma part
J'avais dit que les issues boucle #148/#150/#151/#152 étaient « bloquées par un design différé ». **Faux** : ADR-0011 spécifie déjà la convergence boucle (bounded = barrière de lap via le fan-in du nœud de jointure ; collection = barrière done→Merge ; épuisement = blocage explicite). Ce qui est différé = juste le périmètre du correctif Merge (per-itération). Donc je fais #148 cette nuit (avec gate stop-si-cale) ; #150/#151/#152 attendent un #148 validé + ta revue.

## Ce que je NE fais PAS sans toi (et pourquoi)
- **#150 / #151 / #152** : seulement après un #148 validé (ils s'empilent dessus ; un #148 bancal cascaderait).
- **Éditer les issues GitHub** (« blocked by ») : je scope plutôt les runs par priority/max_waves (même effet, sans toucher ton tracker). Si tu veux quand même la dépendance inscrite, dis-le.
- **Merger quoi que ce soit sur ta branche** : jamais. Tu relis les branches d'intégration au réveil.

## Triggers — LANCÉS (branche séparée)
Vérifié : `docs/triggers-design` (316a172) est une base complète (ADR-0012 + maquette `triggers-screens.jsx`/`triggers.css` + section CONTEXT.md). Triggers orthogonaux au canvas. Batch lancé `wf_62e67f61-4b6`, **sourcé en lecture seule depuis `docs/triggers-design`** (ton checkout intact), scopé aux 5 issues triggers via priority (n'attrape PAS les issues boucle). Deps : #158→#160→{#161,#162}, #159 libre. Livrable sur une branche d'intégration `wf/<tag>/integration` séparée du canvas, à relire indépendamment.

## Boucle restante (#150/#151/#152) — pour toi
NON lancées (volontaire). Elles s'empilent sur #148 ; je les ai gardées pour APRÈS ta revue de #148 (bounded loop region), histoire de ne pas empiler 3 slices sur une fondation non relue. #151 (collection/ForEach) porte aussi la convergence par-lap `collection` (barrière done→Merge) — vérifie l'alignement avec le modèle edge-résolution de #148.

## Ménage à faire (worktrees stale, sans risque, quand tu veux)
Runs antérieurs laissés sur disque : `wf/1780670457-26098/*` (filet run-1), `wf/1780678578-16908/*` (run-2 backend isolé, obsolète), `wf/1780686669-6590/*` (fondation, désormais incluse dans 1780697052). Purgeables une fois 1780697052 relue.

## Journal (horodaté en bas, le plus récent en haut)
- TRIGGERS lancés `wf_62e67f61-4b6` (source=docs/triggers-design lecture seule, priority [158,159,160,161,162], max_waves 5). Dernier batch de la nuit. Veilleur armé (source + vague1=#158).
- #148 PASS → `wf/1780712650-23824/integration` @ 67745eae. Bounded loop region OK (modèle+rendu+15 tests). Gate « stop si cale » : pas calé → mais je m'arrête côté boucle (revue #148 par l'utilisateur avant #150/151/152, comme convenu).
- BATCH CANVAS NON-BOUCLE TERMINÉ : 5/5 PASS (#147,145,149,153,154). Intégration cumulative `wf/1780697052-25210/integration` @ 55e467d. → #148 lancé `wf_aef71f2c-b7d` (gaté, 1 issue, empilé dessus). Après #148 : triggers (voir section dédiée).
- Veilleur batch canvas OK : bootstrap source = wf/1780686669-6590/integration @ 2eaa4ed (option `source` opérationnelle), vague 1 = #147. Le batch tourne ; j'attends sa complétion (ou un STOP sur L5 FAIL). Ensuite : #148 (gaté) puis triggers (conditionnel).
- BATCH CANVAS lancé `wf_68d5ea0b-421` : empilé sur l'intégration (source=wf/1780686669-6590/integration @ 2eaa4ed), priority [147,145,149,153,154], max_waves 5. #147 retire aussi SwitchNode. Veilleur armé (vérifie source + vague1=#147). Gate: STOP_ON_FAIL actif → si un L5 échoue sans fix, le batch s'arrête (pas d'empilement sur base cassée).
- DESIGN refresh committé `2eaa4ed` sur l'intégration (lien expiré 404, mais bundle déjà en cache → utilisé). Superset : refonte-canvas + Node Variants + écrans Triggers. Subagent design-files initial s'était arrêté (404) sans rien casser ; fait inline depuis le cache.
- #144 VALIDÉ PASS (testeur indépendant `ac856c42`) : L5 réelle OK (run live, merge spawn, run completed, pas de halt), Rust 730/0, revue adversariale → le `all_done` backstop est un faux positif (aucun trou). #144 = solide.
- Option `source` ajoutée à la skill (empile un run sur une branche d'intégration donnée, sans toucher le checkout). Parse OK.
- Subagent design-files `ac0d43d4` lancé : remplace `docs/design/` par le nouveau bundle + commit sur la branche d'intégration. APRÈS lui → batch canvas.
- PRÊT (en attente du design-files) : batch canvas via scriptPath, args objet {source: wf/1780686669-6590/integration, priority: [147,145,149,153,154], max_waves: 5, notes}. #147 retire AUSSI SwitchNode (le différé de #144).
- Test subagent indépendant `ac856c42` lancé : valide #144 (edges + merge fix) — L5 réel best-effort + suite Rust + revue adversariale (point chaud : le backstop `all_done` dans lib.rs ~3445, laissé hors-périmètre par l'implémenteur — vrai trou ou pas ?).
- Merge-fix TERMINÉ : commit `da0d72e` sur l'intégration. Barrière Merge edge-centrée + halt explicite. Rust 730/0, scheduler 86/0. Docs (ADR-0006 addendum + ADR-0011 + CONTEXT.md) faites. L5 sautée par l'implémenteur (validée en tests déterministes) → d'où le test subagent indépendant.
- Design-issues subagent TERMINÉ : 10 issues canvas repointées vers `docs/design/project/`. Reste : remplacer les fichiers `docs/design/` (après validation #144) + le batch canvas.
- Lancement : subagent merge-fix `a577fdb8` démarré ; design-issues `a7cf41e1` lancé en parallèle.
