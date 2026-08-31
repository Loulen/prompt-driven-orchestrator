# Node `script` — bash déterministe de l'auteur comme node first-class

Sans cette ADR, un effet de bord déterministe (envoyer une notification Discord au démarrage, #248)
se modélise comme un node agent dont le prompt dit « lance exactement ce bash » : non déterministe,
et un tour de LLM brûlé. CONTEXT.md définissait alors un Node comme « une instance de Claude Code » —
aucun node n'atteignait un état terminal **sans Claude**.

**Décision : ajouter un node `script`, mécanique et productif, qui exécute le bash de l'auteur dans
une session tmux (comme un node agent, mais bash au lieu de `claude`), se complète sur exit 0 /
échoue sinon, et — en v1 — n'obtient pas de sous-worktree (effet doc-only : il tourne dans le
worktree du Run et doit le laisser propre).**

- **Exécution dans tmux, pas hors-bande.** Choisi **contre** l'alternative « tâche async in-daemon » :
  celle-ci donnait une complétion par code de retour propre mais violait l'invariant « tout NodeRun
  est attachable » et ADR-0005, et forçait un garde par type de node sur six sites critiques de
  détection de vie. Rester dans tmux réutilise spawn/attach/reap/complétion/validation/admission tels
  quels, et la session survit à un redémarrage du daemon. Le coût : un script hung serait invisible à
  la détection de vie (pas de transcript JSONL) — d'où un **`timeout` obligatoire dans le wrapper**
  (défaut 60 s, `SCRIPT_TIMEOUT_SECS` ; expiration ⇒ échec).
- **Le corps exécuté est le bash brut, jamais le prompt augmenté.** Un node agent reçoit un préambule
  prose ; un script le « bash »-erait comme du code.
- **Contrat d'I/O par variables d'environnement** (`PDO_INPUT_<PORT>`, `PDO_OUTPUT_<PORT>`,
  `PDO_ARTIFACTS_DIR`, `PDO_VAR_<NAME>`). Le script écrit lui-même son `output.md` ; la validation
  des outputs s'applique mais **fail-fast** — la session a déjà quitté, il n'y a plus d'agent à
  relancer pour un retry interactif. Un input `repeated`/poolé (#353) résout vers **un chemin par
  itération *complétée*** du nœud source — jamais un glob du disque, sinon une itération échouée
  ayant laissé un artefact serait poolée. La liste est **séparée par des sauts de ligne** (un chemin
  peut contenir des espaces), avec `PDO_INPUT_<PORT>_REPEATED=1` posé ; un pool vide → valeur vide,
  flag présent, le script peut `pdo skip`.
- **Corps stocké dans le slot prompt du node**, réutilisé verbatim — aucun nouveau champ de
  sérialisation, réutilisation en bibliothèque gratuite. Un corps vide fait **échouer le lancement**
  (fail-loud), fermant le trou du no-op silencieux.
- **Sécurité.** Le bash d'un script ≡ le guard de Trigger ≡ le bash d'un agent : même surface, aucune
  nouvelle frontière de confiance. C'est le bash *de l'auteur, dans son propre pipeline* — à
  distinguer du JS tiers importé qu'ADR-0016 encadre. Le vrai contrôle reste le binding réseau sans
  auth du daemon (#260), hors scope.

**Alternatives écartées.** *Guard/trigger* — un guard est un prédicat booléen d'edge, pas un node
producteur d'artefact. *Primitive runtime « command » générique (ADR-0009 couche 2)* — invisible dans
l'éditeur visuel, non composable par l'auteur de pipeline.

**Relations.** Étend la famille « mécanique/déterministe » d'ADR-0011 du *routage* à l'*exécution* de
node. Instance la plus tranchante d'ADR-0001 (bash arbitraire). Hérite d'ADR-0005 (tmux). N'est
**pas** contraint par ADR-0008 (un script ne référence aucun champ de frontmatter amont ; il consomme
des artefacts entiers en fichiers). Ne supersede aucun ADR.

**Portée v1 (différé).** Effet code-mutating pour un script (sous-worktree + merge-back) ;
`timeout_secs` configurable par node ; durcissement du garde doc-only contre un commit qui déplace
HEAD.
