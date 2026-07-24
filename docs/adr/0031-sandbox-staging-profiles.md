# ADR-0031 — Profils de staging (contenu du home stagé d'un Run sandboxé)

> Statut : accepted (grilling du 2026-07-24, PRD #403). Vocabulaire : CONTEXT.md § « Sandbox ».
> Complète ADR-0030 (modèle d'exécution) : ADR-0030 dit *où* tourne un Run sandboxé, celle-ci dit
> *avec quel contenu de home*. Implémentée par les slices « plancher » puis « profils » : §1 est
> **réalisé en #426** (avec l'amendement §1 d'ADR-0030) ; §2-§7 restent à livrer.

Le contenu du *staged Claude home* cesse d'être une constante Rust invisible. Il devient un
**profil de staging** : une liste nommée, éditable, sélectionnable par Run et par Trigger.

## Ce qu'on décide

1. **Le plancher est une liste de garanties, pas de fichiers verrouillés** *(réalisé en #426)*. Quel que soit le
   profil, `prepare` garantit qu'au démarrage la session dispose de : credentials valides, managed
   settings de l'org **consentis**, bypass permissions accepté, confiance pré-accordée à la racine
   du Run, `projects/` vide. Chaque garantie est satisfaite **soit** par une entrée du profil,
   **soit** par une synthèse de repli. C'est ce qui rend le décochage sûr sans avoir à l'interdire.
   Formulé en fichiers, le plancher se contredirait dès le premier cas réel : `settings.json` est
   copié depuis l'hôte en `full` mais synthétisé à une seule clé en `minimal`, et un utilisateur qui
   décoche le sien (ses hooks hôte n'existent pas dans le conteneur) doit obtenir la synthèse, pas
   un refus.

2. **Un profil est un *diff* contre le défaut, jamais un instantané.** Le stockage retient
   l'intention de l'utilisateur (`disabled: […]`, `extras: […]`), pas la liste effective. Un
   instantané figerait l'install : le jour où une version de PDO ajoute une entrée au défaut — ce
   que le plancher vient précisément de faire — les profils existants ne la verraient jamais.
   Corollaire : `minimal` et `full` sont des **défauts virtuels** (aucune ligne en base) jusqu'à
   édition ; les éditer matérialise une ligne portant elle aussi un diff.

3. **Une entrée est un chemin relatif à `$HOME`.** `.claude/skills`, `.claude/settings.json`,
   `.gitconfig`, `.config/gh`. Refusés : chemin absolu, `..`, toute sortie de `$HOME`, et
   `projects/` sous `.claude` (puits de transcripts runtime : le copier casserait l'idempotence de
   `merge_back` et le calcul de coût). `.ssh`, `.aws`, `.gnupg` sont **autorisés avec
   avertissement** — les interdire serait du théâtre alors qu'ADR-0030 assume déjà l'uid hôte, le
   repo monté rw et de vraies credentials Claude.

4. **Les entrées hors `.claude` sont copiées puis montées, jamais bind-montées depuis l'hôte.**
   `<staging>/home/<chemin>` → `$HOME/<chemin>`, en **rw**. L'invariant « le vrai `~/.claude` n'est
   jamais monté » s'étend au reste de `$HOME`. Un bind direct exposerait l'hôte à l'écriture du
   conteneur : un agent en `--dangerously-skip-permissions` qui bute sur `unable to auto-detect
   email address` fait très naturellement `git config --global`, et réécrit le `~/.gitconfig` de
   l'utilisateur. Les écritures utiles du conteneur (refresh de token `gh`) sont perdues au
   `teardown` — assumé, `merge_back` ne remonte que les transcripts.
   **Dédup obligatoire** : une entrée sous `.claude/` ne reçoit **pas** son propre `-v`, elle est
   déjà servie par le mount `.claude`. Un double bind serait accepté par Docker et résolu par
   profondeur de chemin — un bug de dimanche.

5. **Le champ sandbox reste une valeur unique : `off` ou un nom de profil.** Pas de liste par Run
   ni par Trigger. La précédence existante (`effective_sandbox` : explicite → Trigger → défaut
   d'instance) ne bouge pas, et les sélecteurs du NewRunModal et du panneau Trigger restent des
   `<select>`. L'alternative — le réglage-liste sur les trois tiers — imposerait le widget d'édition
   à trois endroits et une composition de diffs entre tiers dont aucune sémantique n'est devinable.

6. **Le nom du profil ET la liste résolue sont gelés dans `RunStarted`.** `prepare` lit l'état du
   Run, jamais le réglage vivant. `ensure_ready` est appelé à quatre endroits (création, boot
   recovery, résurrection de session, run-shell) et `prepare` est additif — il copie ou écrase, il
   ne supprime jamais. Sans gel, un daemon redémarré après une édition du profil produirait un home
   incohérent entre deux nœuds du même Run, avec un `plugins/` physiquement présent malgré son
   décochage. Le gel de la **liste** en plus du **nom** évite en outre qu'éditer un profil réécrive
   rétroactivement ce qu'un Run passé a stagé.

7. **Un nom de profil inconnu échoue fort, partout.** 400 à la création de Run, échec visible du tir
   de Trigger, `RunFailed` explicite en boot recovery. Jamais de retombée silencieuse sur le défaut
   d'instance — le comportement que produirait naturellement le `parse() → None` actuel, et que
   l'ADR-0030 §4 interdit déjà pour l'indisponibilité de Docker. Côté UI, supprimer un profil
   référencé liste ses référents avant confirmation : garde-fou souple, pas d'intégrité
   référentielle en base.

## Pourquoi (ce que le mode seul ne pouvait pas faire)

Le mode est un interrupteur à deux positions qui décide de *tout* d'un coup — skills, plugins,
agents, commands, settings, `.md` globaux. Or le poste de coût est **un seul** de ces éléments :
`full` pèse ~1 Go par Run, « dominé par `plugins/*/node_modules` », et le staging n'est purgé
qu'au `cleanup_run`. Un pipeline qui a besoin des skills mais pas des serveurs MCP n'avait aucune
option : il payait 1 Go ou il perdait tout. Sur une instance à Triggers horaires, ce choix
binaire alimente directement la récurrence disque connue.

Symétriquement, le staging ne pouvait transporter **que** du `~/.claude`, alors que ce qui manque à
un Run sandboxé pour faire le travail réel est ailleurs dans `$HOME` : l'identité git est globale
(`~/.gitconfig`), donc un agent qui commite dans le conteneur échoue ; l'auth `gh` vit dans
`~/.config/gh`. Le profil résout les deux besoins avec un seul concept.

## Alternatives écartées

- **Extras seulement, sans décochage.** Simple, mais ne règle pas le poste de coût — le seul
  élément qu'on veuille vraiment retirer est dans le défaut.
- **Override complet de la liste.** L'utilisateur réécrit tout ; le plancher est réinjecté. Plus
  souple, moins découvrable, et prive l'install des évolutions futures du défaut.
- **Réglage-liste sur les trois tiers** (Run / Trigger / instance). Écarté en §5.
- **Manifeste dans le staging dir** plutôt que gel dans `RunStarted`. Évite un champ de payload,
  mais éclate le contrat d'isolation d'un Run sur deux stockages, et disparaît au `teardown`.
- **Interdire `.ssh`/`.aws`/`.gnupg`.** Incohérent avec la posture v1 d'ADR-0030 (cf. §3).

## Limites acceptées

- Le blast radius filesystem n'est plus « rien d'autre que `.claude` » mais « ce que le profil
  déclare ». Le refus par défaut de `$HOME` devient une **liste d'exceptions déclarées et
  visibles** — voir l'amendement d'ADR-0030.
- Les profils vivent en base, pas sur disque : ils ne se versionnent pas avec le repo. Assumé, un
  profil référence des chemins spécifiques à la machine. Le Dockerfile, lui, reste sur disque
  précisément parce qu'il est fait pour être partagé.
- Une édition de profil ne rattrape pas les Runs en vol (conséquence directe du gel, §6).

## Relations

- **ADR-0030** — modèle d'exécution ; amendé pour les mounts d'exception `$HOME` et l'échec fort.
- **ADR-0015** — précédence `stored → env → default` des réglages d'instance ; les défauts virtuels
  `minimal`/`full` en sont l'application à une valeur non scalaire.
- **ADR-0001** — outil tranchant, pas outil sûr : fonde le choix « autoriser + avertir » (§3).
- **#403** — PRD Sandbox ; ces décisions sont livrées par les slices post-validation du PRD. §1 est
  livré par **#426**, §2-§7 par les slices « profils ».
