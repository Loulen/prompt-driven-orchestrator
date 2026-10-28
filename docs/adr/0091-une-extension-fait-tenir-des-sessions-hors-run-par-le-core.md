# Une Extension fait tenir des sessions hors Run par le core, qui ne touche jamais leur dossier

> Statut : accepted (grilling du 2026-10-28, CORE-121). Vocabulaire : CONTEXT.md § *Session
> hébergée*.

## Contexte

Une Extension veut offrir des agents de longue durée, ou des shells, qu'un humain démarre à la
main et garde sous la main : sans start ni end, sans entrée ni sortie, éventuellement hors de
tout dépôt. Le Run est l'exécution d'un Pipeline, dans les worktrees d'au moins un dépôt ; le
core ne tient hors Run que l'Assistant de bibliothèque, interne. Un lancement de Node écrit dans
son dossier de travail (prompts, hooks, skills) et lit l'instantané de son Run.

## Décision

1. **La Session hébergée est un genre de session du core**, ouvert aux Extensions par l'Hôte :
   ouvrir, relancer, terminer, configurer, y taper du texte, lire, lister, oublier. Chaque geste
   est une Opération sur la Ressource `session`, rattachée au Projet de la session s'il y en a
   un, et chaque changement d'état un Événement de session.
2. **Le core enregistre chaque Session hébergée** (Extension, clé, genre, configuration, dossier,
   Projet, Acteur qui la démarre, identifiant de conversation, Zoom du terminal) : elle survit au
   redémarrage du daemon avec son démarreur, le balayage ne la tue jamais, aucun TTL ne s'y
   applique. Elle s'arrête sur « terminer » ou à la sortie de son processus, et se relance en
   reprenant la conversation quand le harnais le permet.
3. **Le core n'écrit jamais dans son dossier de travail** : amorce, hooks et Artefacts live vont
   dans un dossier d'état sous `~/.pdo/`. Elle ne reçoit donc **aucun skill** : la livraison
   d'ADR-0062 écrit dans le dossier, et ce dossier peut être le home (les skills globaux du
   harnais) ou un dépôt où un humain travaille.
4. **Elle ne se complète pas** : `pdo wait-user` y déclare l'attente comme pour un Node, Entrée
   la lève ; `pdo complete` y est refusé.
5. **Le core n'y ajoute aucun préambule** : l'agent ne lit que le texte de l'Extension
   (Contribution au lancement) et le prompt initial. Les notions de Run, de Pipeline, d'output
   ou de complétion n'ont rien à y faire ; l'Extension dit elle-même ce qu'elle veut (Artefacts
   live, attente déclarée), le core fournit seulement les variables (`PDO_SESSION_ID`,
   `PDO_LIVE_DIR`, `PDO_DAEMON_URL`).
6. **Le Cap de sessions concurrentes la compte** : moins éphémère que le Manager ou
   l'Assistant, elle tient un harnais aussi longtemps qu'un Node.
7. **L'UI se livre en briques** : `@pdo/extension-ui` exporte le terminal de session et les
   éléments d'un détail de session (en-tête, gestes, configuration, Zoom du terminal, Artefacts
   live, Événements, onglets `session/detail`) ; l'Extension compose sa mise en page.

## Alternatives écartées

- **Un Run par session**, pipeline à un seul Node interactif : il exige un dépôt, un start et un
  end, crée un worktree, et remplit la liste des Runs ; le masquer ou le créer sans dépôt sont
  autant d'exceptions au Run.
- **Un Run portant plusieurs sessions**, Nodes à la demande ajoutés à chaud : sans dépôt, sans
  start ni end, un dossier par Node, masqué des listes ; chaque exception touche tout ce qui lit
  un Run (Stats, coût, Review), et les sessions partageraient un worktree.
- **Livrer les skills dans le dossier** comme pour un Node : écrit dans les skills globaux du
  harnais quand le dossier est le home, et dans l'arbre d'un humain qui travaille dans le même
  dépôt ; un pointeur propre au harnais (`--plugin-dir`) reste écarté par ADR-0062.
- **Un panneau de session exporté d'un bloc** : l'Extension qui l'héberge ne pourrait pas le
  remanier ; les briques couvrent le même besoin et laissent la mise en page à l'Extension.
