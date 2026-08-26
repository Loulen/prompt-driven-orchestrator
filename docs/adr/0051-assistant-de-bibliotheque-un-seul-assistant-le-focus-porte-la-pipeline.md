# Assistant de bibliothèque : un seul assistant, le focus porte la pipeline

> Statut : **accepted** (grilling du 2026-08-27, issue #594). **Amende ADR-0048** : ses décisions 1
> (session keyée sur la pipeline), 3 (create-on-open / reap-on-leave) et 4 (jamais reapée par le
> sweep) sont remplacées par celles ci-dessous ; le reste d'ADR-0048 (mécanisme de session, write-on-save,
> pas de MCP custom) tient. Vocabulaire : CONTEXT.md §*Assistant de bibliothèque*.

L'assistant livré par #302 était **un `claude` par pipeline**, dont la durée de vie était celle de
l'onglet **Assistant** affiché. À l'usage, ça ne colle pas au travail réel : on édite une pipeline,
on va en voir une autre, on revient — et à chaque aller-retour on jetait la conversation pour en
rallumer une qui ne savait rien. Le propriétaire l'a résumé en quatre reproches (#594), dont deux
qui se contredisent en apparence : « reap trop lent » et « ne pas reap tant qu'on édite ». Ils ne se
contredisent pas : ils disent que **l'unité de durée de vie était la mauvaise**.

## Ce qu'on décide

1. **Un seul assistant par daemon**, plus un par pipeline. Session `pdo-libassist-shared`, cwd = le
   dossier des templates du repo. La pipeline n'est plus dans le nom de la session, ni dans le cwd,
   ni dans le primer.

2. **Le focus est un état du daemon, pas un argument de spawn.** L'UI déclare en continu la pipeline
   qu'elle édite (id + scope) ; le daemon la garde en mémoire, horodatée. Cet état sert deux fois :
   il dit à l'assistant sur quoi il travaille, et il dit au sweep si un humain est encore là.

3. **La conscience de la pipeline ouverte est un `UserPromptSubmit` hook, pas une consigne de
   prompt.** Le hook lit le focus et l'injecte dans le contexte à *chaque* message — donc sans
   dépendre de la discipline du modèle. Le primer garde la consigne équivalente en clair, parce que
   le hook n'existe que sur un harnais qui expose `--settings` (le `claude` de la registry, pas
   `opencode` ni `pi`) : sur les autres, la consigne est le seul mécanisme, et on l'assume dégradée
   plutôt qu'absente. Même discipline qu'ADR-0043 : on pose un hook via `--settings`, on ne ship pas
   de MCP custom (ADR-0048 §5 tient).

4. **Le reap est conditionné à l'absence de l'humain, plus à l'affichage d'un onglet.** Trois
   verdicts, dans cet ordre : une session **attachée** (un terminal ouvert dans un navigateur) n'est
   jamais tuée ; un focus **frais** (l'utilisateur est sur une vue d'édition, même sans l'onglet
   Assistant affiché) n'est jamais tué ; sinon le sweep la tue après une TTL d'inactivité courte
   (défaut 120 s, sur la cadence existante du reaper). Le `DELETE` explicite reste, mais déclenché
   quand on quitte **toute** vue d'édition — plus quand on quitte l'onglet.

## Pourquoi pas les alternatives

- **Garder l'exemption inconditionnelle du sweep (ADR-0048 §4) et se contenter d'accélérer le
  `DELETE`.** Le journal du daemon dit que le `DELETE` n'est déjà pas lent : sur les huit sessions
  assistant enregistrées depuis la livraison, chaque `Spawned` a son `Reaped` apparié, la plus
  longue ayant vécu 6 min 55 s **avec son WebSocket PTY ouvert de bout en bout** — donc affichée. Le
  seul cas réellement cassé est ailleurs et n'est pas lent mais **non borné** : React ne joue pas ses
  cleanups au déchargement du document, donc un reload ou une fermeture d'onglet n'envoie jamais le
  `DELETE`, et rien d'autre ne reapait un `pdo-libassist-*`. Sans filet côté serveur, la session
  survit jusqu'au prochain open+leave de la même pipeline, c'est-à-dire potentiellement jamais.
  C'est cette absence de filet qui coûte l'exemption inconditionnelle.
- **Déduire la présence de l'attachement tmux seul.** `#{session_attached}` est un signal serveur
  honnête et gratuit, mais il ne répond pas à la question du propriétaire : on veut survivre *pendant
  qu'on édite*, y compris quand l'onglet Assistant n'est pas affiché — et le panneau info se ferme
  tout seul à chaque changement d'onglet d'édition, donc l'attachement tombe. Il reste comme
  garde-fou (verdict 1), pas comme oracle.
- **Une présence sur le WebSocket d'événements.** Il faudrait le rendre bidirectionnel pour des
  faits qu'un `PUT` HTTP transporte déjà, et la mort d'un pair TCP à moitié fermé se constate en
  minutes : latence de reap non prédictible.
- **Injecter la pipeline courante dans la REPL par `tmux send-keys`.** C'est le seul précédent
  existant d'injection (la boucle corrective de frontmatter), et il ne vaut que pour un agent qu'on
  sait au repos : si l'utilisateur est en train de taper, le `send-keys ... Enter` s'insère dans sa
  phrase et la soumet. On ne pousse pas dans une REPL qu'un humain pilote.
- **Détecter la soumission d'un message en reniflant le flux PTY.** Le pont est un tuyau bête par
  décision (ADR-0021) ; un `\r` ne distingue pas un message soumis d'un Entrée dans une boîte de
  dialogue, d'un collage entre crochets ou d'un Shift+Entrée.

## Conséquences

- **Une seule conversation, donc un seul historique**, partagé entre toutes les templates : c'est le
  gain (le contexte survit à un aller-retour) et le coût (pas d'isolation entre deux pipelines qu'on
  édite en alternance ; on l'accepte, l'assistant a le focus à chaque message pour se resituer).
- **La fuite au redémarrage-du-daemon-onglet-fermé** qu'ADR-0048 assumait disparaît : le sweep
  reprend la main sur cette session, focus périmé = mort.
- **Le cwd de l'assistant était faux** pour un onglet de scope `repo` ou `user` (il pointait sur le
  *library store*, où le `<id>.yaml` annoncé par le primer n'existe pas). En sortant la pipeline du
  cwd, le bug disparaît : le focus porte le chemin absolu du fichier réellement ouvert.
- Le save doit désormais nommer son `scope` explicitement : un assistant unique traverse les scopes,
  et le défaut côté daemon (`repo`) migrerait silencieusement une template `user`.
