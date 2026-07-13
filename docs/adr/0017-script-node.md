# Node `script` — bash déterministe de l'auteur comme node first-class

Aujourd'hui, *tout* node qui exécute (doc-only, code-mutating, merge) lance une
session Claude Code dans tmux et se signale terminé via `pdo complete`
(`node_primitives::start_node`). CONTEXT.md définit d'ailleurs un Node comme « une
instance de Claude Code ». Or #248 (pipeline « discord chat ») veut un effet de bord
**déterministe** — envoyer une notification Discord au démarrage — sans dépenser un
tour de LLM. Le node `Merge` mécanique décrit par ADR-0006 n'a jamais été câblé
(`merge_action.rs` est du code mort) : il n'existe donc, à ce jour, aucun node qui
produise du travail et atteigne un état terminal **sans Claude**.

**Décision : ajouter un node `script`, mécanique et productif, qui exécute le bash de
l'auteur dans une session tmux (comme un node agent, mais bash au lieu de `claude`),
se complète sur exit 0 / échoue sinon, et — en v1 — n'obtient pas de sous-worktree
(effet doc-only : il tourne dans le worktree du Run et doit le laisser propre).**

- **Exécution dans tmux, pas hors-bande.** Le tail de `build_tmux_script` bascule vers
  un wrapper `timeout N bash <corps>; exit-code → pdo complete|fail` au lieu de
  `exec claude`. Choisi **contre** l'alternative « tâche async in-daemon (clone de
  `guard_runner`) » : cette dernière donnait une complétion par code de retour propre
  mais violait l'invariant « tout NodeRun est attachable » (CONTEXT.md, *Deliberate,
  then autonomous*) et ADR-0005 (observabilité par xterm inline), et forçait un
  garde `node_type=="script"` sur six sites critiques de détection de vie
  (`stale_detector`, `boot_recovery` ×2, diagnostics de mort de session, `admission`,
  bridge PTY frontend). Rester dans tmux réutilise spawn/attach/reap/`node_done`/
  `outputs_validator`/admission tels quels, et la session survit à un redémarrage du
  daemon (tmux est un process séparé). Le coût : la complétion passe par le wrapper
  (pas d'attente directe du code de retour) et un script hung sans `timeout` serait
  invisible à la détection de vie (pas de JSONL) — d'où le `timeout` obligatoire dans
  le wrapper (défaut 60 s, `SCRIPT_TIMEOUT_SECS`, `timeout` sort 124 ⇒ échec).
- **Le corps exécuté est le bash brut, pas le prompt augmenté.** Un node agent reçoit
  un préambule prose (`build_full_prompt`) ; un script `bash`-erait ce préambule comme
  du code. Le spawn écrit donc le **`role_prompt` brut** (le corps) dans le fichier
  que `bash` exécute, jamais le prompt augmenté.
- **Contrat d'I/O par variables d'environnement.** Un script ne lit pas le préambule
  prose des agents. Le runtime injecte `PDO_INPUT_<PORT>`, `PDO_OUTPUT_<PORT>`,
  `PDO_ARTIFACTS_DIR`, `PDO_VAR_<NAME>` (en plus des `PDO_RUN_ID/NODE_ID/NODE_ITER/
  DAEMON_URL` déjà présents). Le script écrit lui-même son `output.md` à
  `$PDO_OUTPUT_<port>` (frontmatter compris s'il veut piloter une edge `when:`) ;
  `outputs_validator` s'applique, mais **fail-fast** (pas de retry interactif : la
  session a déjà quitté, il n'y a plus d'agent à relancer). Les répertoires des ports
  de sortie sont **pré-créés au spawn** (un `> "$PDO_OUTPUT_out"` échouerait sur un
  parent manquant). Un input `repeated` arrive comme la **liste séparée par des
  sauts de ligne** des chemins des itérations COMPLETED de la source (plus
  `PDO_INPUT_<PORT>_REPEATED=1`), **pas** un glob `iter-*` : un glob se ré-étendrait
  sur disque dans le bash du script et ré-inclurait les itérations échouées mises en
  quarantaine (#353). Le script itère p. ex. `while IFS= read -r f; do …; done <<<
  "$PDO_INPUT_<PORT>"` (les valeurs env sont single-quotées par `wrap_with_env`, donc
  les sauts de ligne survivent ; pool vide ⇒ variable vide).
- **Le seam de test `tmux_cmd_override` est contourné pour un script.** Pour un agent,
  l'override remplace `claude` par un stub pour que la CI ne lance jamais de vrai
  claude. Un script *est* du bash déterministe : l'override ne doit pas l'écraser.
  Conséquence (une force) : un node script est testable de bout en bout en CI **sans
  aucun stub** — une propriété strictement plus forte que n'importe quel node agent.
- **Corps stocké dans le slot prompt du node** (`<pipeline>.prompts/<node>.md`),
  réutilisé verbatim — aucun nouveau champ de sérialisation, aucun changement de
  watcher, réutilisation en bibliothèque gratuite. Un corps vide fait **échouer le
  lancement** (fail-loud, comme une edge pendante), fermant le trou du no-op silencieux.
- **Sécurité.** Le bash d'un script ≡ le guard de Trigger (`sh -c` par le daemon) ≡
  le bash d'un agent (via `claude`) : même surface, aucune nouvelle frontière de
  confiance. C'est le bash *de l'auteur, dans son propre pipeline* — à distinguer du
  JS tiers importé qu'ADR-0016 encadre. Le vrai contrôle reste le binding `0.0.0.0`
  sans auth du daemon (#260), hors scope ici.

**Alternatives écartées.** (A) *Pas de nouveau type ; un node code-mutating dont le
prompt dit « lance exactement ce bash »* — non déterministe, brûle un tour de LLM,
rate l'objectif. (B) *Modéliser l'étape comme un guard/trigger* — un guard est un
prédicat booléen d'edge, pas un node producteur d'artefact. (C) *Une primitive
runtime « command » générique (ADR-0009 couche 2)* — invisible dans l'éditeur visuel,
non composable par l'auteur de pipeline. (D) *Tâche async in-daemon (clone de
`guard_runner`)* — voir la décision ci-dessus (violait ADR-0005 + 6 gardes de vie).

**Relations.** Étend la famille « mécanique/déterministe » d'ADR-0002 du *routage* à
l'*exécution* de node. Instance la plus tranchante d'ADR-0001 (bash arbitraire).
Hérite d'ADR-0005 (tmux). N'est **pas** contraint par ADR-0008 (un script ne
référence aucun champ de frontmatter amont ; il consomme des artefacts entiers en
fichiers). Ne supersede aucun ADR. Le code mort `merge_action.rs` (divergence
ADR-0006) reste hors scope (follow-up).

**Portée v1 (différé).** Effet code-mutating pour un script (sous-worktree +
merge-back) ; `timeout_secs` configurable par node ; durcissement du garde doc-only
contre `git commit` (contrôle de non-déplacement de HEAD).
