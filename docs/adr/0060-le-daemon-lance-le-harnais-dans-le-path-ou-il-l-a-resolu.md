# ADR-0060 — Le daemon lance le harnais dans le `PATH` où il l'a résolu

> Statut : accepted (bug #661, « faux session_died : la session tmux lance le harnais dans le `PATH`
> du service »). Vocabulaire : CONTEXT.md § « Harnais agentique ». Prolonge ADR-0055 : la résolution
> et le lancement partagent désormais le même `PATH`. Touche ADR-0019, ADR-0037.

## Contexte

ADR-0055 a fait résoudre les binaires de harnais dans le `PATH` **interactif** de l'utilisateur
(`harness_probe_path()`), parce qu'un service n'hérite pas de l'environnement utilisateur. Mais ce
`PATH` n'était lu que par les **sondes** — le préflight `binary_available`, la sonde de version, le
catalogue. **Il n'était jamais exporté dans la session qui lance le binaire.**

Le tail de session est `exec bash -c '… claude …'` (`wrap_with_env`). Un `bash -c` n'est ni
interactif ni de login : il ne source rien, donc il tourne dans le `PATH` du serveur tmux, hérité du
daemon, c'est-à-dire celui de l'unité de service (`build_path_env`, ADR-0019). Quand `claude` est
posé par l'installeur natif sous `~/.local/bin`, absent de ce `PATH`, le lancement sort en `127`, la
fenêtre unique rend la main, la session (puis le serveur tmux) tombe, et le sweep de liveness écrit
`session_died` — un échec qui **nomme tmux alors que la vraie faute est un `PATH`** (exactement la
mésattribution que ADR-0037 existe pour empêcher).

On résolvait donc dans un `PATH` et on lançait dans un autre. Le préflight passait au vert (il lisait
le bon `PATH`), ce qui explique l'absence de `spawn_aborted: binary not found` : le garde-fou de
ADR-0037 était satisfait par un `PATH` que la session ne recevait pas.

## Ce qu'on décide

**Le daemon exporte le `PATH` de résolution comme `PATH` de la session, en tête du wrapper de
`wrap_with_env`.** La valeur vient de `harness_probe_path()` — le même `PATH` que le préflight — et
elle est *threadée* depuis les seams de spawn (`spawn`, `spawn_shell`, `spawn_libassist`, `resume`),
de sorte que les builders restent purs et leurs goldens hermétiques. Résoudre et lancer partagent le
même `PATH` : l'asymétrie disparaît, et le préflight redevient le garde-fou honnête qu'il prétendait
être.

- **Périmètre : toute session env-wrappée**, pas seulement un nœud agent. Un shell de run existe pour
  taper `claude`/`git`/`pdo` à la main (CONTEXT.md § « Shell de run ») ; il a besoin du `PATH`
  interactif autant qu'un nœud. L'export vit dans `wrap_with_env`, que tous les tails traversent.
- **Sandbox** : l'export tourne côté **hôte** du `docker exec` (#447) et ne traverse pas dans le
  conteneur, donc inoffensif côté conteneur — et utile côté hôte pour retrouver le binaire `docker`
  lui-même s'il vient d'un préfixe utilisateur.
- **`pdo` reste sur le `PATH` de la session** : `harness_probe_path()` fait déjà l'union avec le
  `PATH` du process (dont le répertoire de l'exe, via `build_path_env`), donc le hook de fin de tour
  (`pdo complete --auto`) résout toujours.
- **Un `PATH` vide n'exporte rien** : jamais `export PATH=''`, qui effacerait le `PATH` et casserait
  jusqu'à `bash`/`pdo`. C'est aussi l'échappatoire de byte-identité pour les goldens de forme
  historique.

## Les alternatives écartées

**Enrichir le `PATH` de l'unité de service à sa génération.** Déjà écartée par ADR-0055 : un `PATH`
figé est périmé, et un harnais installé **après** la génération de l'unité reste invisible — le cas
normal. Bon comme contournement machine (`Environment=PATH=…` + `daemon-reload`), pas comme correctif.

**Faire lire `harness_probe_path()` directement par `wrap_with_env`.** Rendrait le builder impur et
ses goldens non hermétiques. On garde l'impureté aux seams et on thread la valeur.

**Corriger seulement le message `session_died`.** Traite le symptôme (la mésattribution) sans la
cause : le lancement échouerait toujours en `127`. La cause est le `PATH`, pas le libellé.

## Limites acceptées

- **Le `PATH` exporté est celui résolu au démarrage du daemon** (cache `OnceLock` de ADR-0055) : un
  changement côté utilisateur n'est vu qu'à la prochaine re-résolution. Même contrainte que le
  catalogue de modèles (ADR-0053).
- **Résoudre le `PATH` interactif source les fichiers de configuration du shell**, coût et effets de
  bord compris — payé une fois, mis en cache, comme sous ADR-0055.

## Antériorité

ADR-0055 (résolution des binaires de harnais dans le `PATH` de l'utilisateur), ADR-0037 (échec de
spawn avant tout effet de bord, jamais un faux 2xx), ADR-0019 (`PATH` de l'unité de service).
