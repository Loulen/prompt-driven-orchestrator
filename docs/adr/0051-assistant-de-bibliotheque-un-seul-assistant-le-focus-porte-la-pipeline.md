# Assistant de bibliothèque : un seul assistant, le focus porte la pipeline

Sans cet ADR, un agent garderait un assistant `claude` par pipeline, dont la durée de vie est celle
de l'onglet affiché — jetant la conversation à chaque aller-retour entre deux templates, et laissant
fuir la session quand le navigateur se recharge.

> Statut : **accepted** (grilling du 2026-08-27, issue #594). **Amende ADR-0048** : ses décisions 1,
> 3 et 4 sont remplacées par celles ci-dessous ; le reste d'ADR-0048 (mécanisme de session,
> write-on-save, pas de MCP custom) tient. Vocabulaire : CONTEXT.md §*Assistant de bibliothèque*.

Le propriétaire a résumé le problème en deux reproches qui se contredisent en apparence : « reap trop
lent » et « ne pas reap tant qu'on édite ». Ils disent la même chose : **l'unité de durée de vie
était la mauvaise**.

## Ce qu'on décide

1. **Un seul assistant par daemon**, plus un par pipeline. Session `pdo-libassist-shared`, cwd = le
   dossier des templates du repo. La pipeline n'est plus dans le nom de la session, ni dans le cwd,
   ni dans le primer.

2. **Le focus est un état du daemon, pas un argument de spawn.** L'UI déclare en continu la pipeline
   qu'elle édite (id + scope) ; le daemon la garde en mémoire, horodatée. Cet état sert deux fois :
   il dit à l'assistant sur quoi il travaille, et il dit au sweep si un humain est encore là.

3. **La conscience de la pipeline ouverte est un `UserPromptSubmit` hook, pas une consigne de
   prompt.** Le hook lit le focus et l'injecte à *chaque* message — donc sans dépendre de la
   discipline du modèle. Le primer garde la consigne équivalente en clair, parce que le hook n'existe
   que sur un harnais qui expose `--settings` : sur les autres, la consigne est le seul mécanisme, et
   on l'assume dégradée plutôt qu'absente. Même discipline qu'ADR-0043 : un hook via `--settings`,
   pas de MCP custom.

4. **Le reap est conditionné à l'absence de l'humain, plus à l'affichage d'un onglet.** Trois
   verdicts, dans cet ordre : une session **attachée** n'est jamais tuée ; un focus **frais**
   (l'utilisateur est sur une vue d'édition, même sans l'onglet Assistant affiché) n'est jamais tué ;
   sinon le sweep la tue après une TTL d'inactivité courte (défaut 120 s). Le `DELETE` explicite
   reste, mais déclenché quand on quitte **toute** vue d'édition.

## Pourquoi pas les alternatives

- **Garder l'exemption inconditionnelle du sweep (ADR-0048 §4) et accélérer le `DELETE`.** Mesuré :
  le `DELETE` n'est pas lent (huit sessions enregistrées, chaque `Spawned` apparié à son `Reaped`).
  Le seul cas cassé n'est pas lent mais **non borné** : React ne joue pas ses cleanups au
  déchargement du document, donc un reload ou une fermeture d'onglet n'envoie jamais le `DELETE`, et
  rien d'autre ne reapait un `pdo-libassist-*`. C'est cette absence de filet qui coûte l'exemption.
- **Déduire la présence de l'attachement tmux seul.** `#{session_attached}` est honnête et gratuit,
  mais on veut survivre *pendant qu'on édite*, y compris quand l'onglet Assistant n'est pas affiché —
  et le panneau info se ferme à chaque changement d'onglet, donc l'attachement tombe. Il reste
  garde-fou (verdict 1), pas oracle.
- **Une présence sur le WebSocket d'événements.** Il faudrait le rendre bidirectionnel pour des faits
  qu'un `PUT` HTTP transporte déjà, et la mort d'un pair TCP à moitié fermé se constate en minutes.
- **Injecter la pipeline courante dans la REPL par `tmux send-keys`.** Le seul précédent d'injection
  ne vaut que pour un agent qu'on sait au repos : si l'utilisateur tape, le `send-keys … Enter`
  s'insère dans sa phrase et la soumet. On ne pousse pas dans une REPL qu'un humain pilote.
- **Détecter la soumission d'un message en reniflant le flux PTY.** Le pont est un tuyau bête par
  décision (ADR-0021) ; un `\r` ne distingue pas un message soumis d'un Entrée dans une boîte de
  dialogue, d'un collage entre crochets ou d'un Shift+Entrée.

## Conséquences

- **Une seule conversation, donc un seul historique**, partagé entre toutes les templates : c'est le
  gain (le contexte survit à un aller-retour) et le coût (pas d'isolation entre deux pipelines
  éditées en alternance ; l'assistant a le focus à chaque message pour se resituer).
- **La fuite au redémarrage-du-daemon-onglet-fermé** qu'ADR-0048 assumait disparaît : focus périmé =
  mort.
- **Le cwd de l'assistant était faux** pour un onglet de scope `repo` ou `user` (il pointait sur le
  *library store*, où le `<id>.yaml` annoncé par le primer n'existe pas). En sortant la pipeline du
  cwd, le bug disparaît.
- **Le save de l'assistant ne nomme plus rien** — ni id, ni scope : `POST /sessions/libassist/save`
  écrit dans le fichier que le focus désigne. La première version faisait réémettre le scope vers
  `POST /library/pipelines`, intenable : cet endpoint lit `scope` dans le vocabulaire du *library
  store*, pas dans celui d'un onglet d'édition ; un assistant parfaitement obéissant écrivait un
  doublon dans l'autre arbre et annonçait « Sauvé » (FP-6). Supprimer l'argument supprime la classe
  de bug : le daemon est le seul à connaître les deux vocabulaires. Il diffuse lui-même le
  `pipeline_changed` qui fait relire le canvas — le watcher de fichiers ne peut pas s'en charger (il
  ignore ses propres écritures et ne surveille pas le library store).
- **Le `DELETE` vide le focus par le même geste.** « Plus aucune vue d'édition ouverte » est le seul
  fait que les deux portent, et le séparer les faisait diverger : sur `pagehide` on ne peut envoyer
  qu'une requête `keepalive`, donc la session mourait et le focus restait — `GET …/focus` nommait une
  template que personne n'avait plus ouverte, d'un âge croissant sans borne.
- **« Quitter toute vue d'édition » se lit sur les onglets ouverts, pas sur l'onglet actif.** Aller
  voir un Run pendant qu'une template reste ouverte n'est pas quitter l'édition ; y reaper la session
  coûterait la conversation, c'est-à-dire le reproche à l'origine de l'issue.
