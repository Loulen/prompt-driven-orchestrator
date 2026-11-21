# L'Agent de support est une session du core par Acteur, hors Run, avec les Droits de l'Acteur

> Statut : accepted (grilling du 2026-11-20). Numéro attribué à la fusion vers `main`.
> Vocabulaire : CONTEXT.md § *Agent de support*.

## Contexte

Pour poser une question sur son instance (« pourquoi ce Run a échoué ? ») ou sur l'usage de PDO
(« comment créer un Trigger ? »), un utilisateur n'a aucun agent à portée de main. Un Run exige
un dépôt cible et une Pipeline ; l'Assistant de bibliothèque ne sert qu'à éditer des Pipelines ;
une Session hébergée (ADR-0092) est ouverte par une Extension, pas par le core. Les Extensions
installées peuvent être closed source : leur code n'est pas là pour qu'un agent le lise.

## Décision

1. **L'Agent de support est une session du core, une par Acteur, hors de tout Run.** Son nom
   dérive de l'Acteur ; l'ouvrir alors qu'il vit est refusé, sauf à demander de le remplacer, qui
   le ferme une fois le nouveau choix résolu. Routes : `POST /sessions/support` (ouvrir : le
   choix d'agent, la question, `replace`), `GET` (état : vivant, agent, dernier choix),
   `POST /sessions/support/ask` (taper une question dans la session vivante), `DELETE` (fermer).
   Commande : `pdo support open|ask|status|close` (ADR-0097).
2. **Il agit avec les Droits de l'Acteur qui l'ouvre.** Ses hooks de lancement sont demandés avec
   lui comme Acteur qui démarre la session (genre `support_agent`) : une Extension d'identité lui
   remet le jeton de cet Acteur, comme au Node d'un Run qu'il a lancé, sans restriction de
   Projet. Ouvrir, interroger et fermer sont des Opérations sur sa session (`session` ×
   `create`, `ask`, `delete`).
3. **Il tourne sur un agent choisi**, un Profil agentique ou une combinaison Custom (jamais
   « Inherit » : il n'a aucun tier dont hériter), dont le harnais doit être installé sur la
   machine ; sinon le refus dit pourquoi. Sans modèle, le défaut de l'instance pour ce harnais
   s'applique. Le dernier choix de l'Acteur est retenu dans son dossier.
4. **Il lit un prompt court embarqué dans le binaire** (une page au plus : son rôle, la
   confirmation avant toute mutation, où chercher), écrit comme ses instructions dans son
   dossier : `AGENTS.md`, que les harnais chargent au démarrage, et `CLAUDE.md`, qui l'importe
   (`@AGENTS.md`) pour claude. Seule la première question est son premier message : la
   conversation commence par elle, le prompt n'y est pas recopié. Sa documentation est la CLI
   du core et des Extensions (`pdo help`, `pdo ext <id> help`) : aucune base documentaire n'est
   rédigée pour lui.
5. **Il travaille dans un dossier par Acteur sous le répertoire d'état de PDO**
   (`~/.pdo/support/<session>/`), que le core prépare à l'ouverture. Le balayage ne le tue pas :
   il s'arrête quand son Acteur le ferme.

## Conséquences

- La palette (Ctrl+K) a un groupe « Support » : ouvrir l'agent, lui poser la question tapée,
  interroger l'agent ouvert sans fenêtre, en ouvrir un autre. L'agent s'affiche dans le Terminal
  agrandi, avec « End session ».
- L'API d'Extension gagne `SessionKind::SupportAgent` : une Extension qui contribue aux sessions
  d'agent le voit passer, avec l'Acteur qui l'ouvre.
- Un agent sans saisie de son Acteur se ferme : au terme du délai d'inactivité de l'instance
  (« Support agent idle timeout », 1800 s par défaut), le daemon publie
  `support_agent_changed` sur `/ws` et chaque onglet de l'Acteur demande « Still there? » ;
  « Keep » (`POST /sessions/support/keep`, `pdo support keep`) ou toute saisie relance le délai ;
  sans réponse en 60 s, le daemon ferme la session, même sans onglet ouvert. Le délai se mesure en
  mémoire du daemon : un redémarrage le fait repartir de zéro, jamais plus court.
- claude, copilot et pi chargent ces fichiers depuis un dossier hors dépôt git (vérifié le
  2026-11-21) ; opencode les lit selon sa documentation, sans vérification. Un harnais déclaré par
  l'utilisateur qui ne charge ni `AGENTS.md` ni `CLAUDE.md` démarre sans ses instructions.
- Une question qui commence par `-` reçoit une espace en tête : le harnais ne la lit pas comme
  une de ses options.
- L'agent tourne avec la ligne de lancement de son harnais, celle des Nodes : pour claude,
  `--dangerously-skip-permissions`. La confirmation avant toute mutation ne repose que sur ses
  instructions ; rien ne l'impose techniquement. À revoir si un agent conversationnel doit
  demander la permission de chaque commande.

## Alternatives écartées

- **Un Run « conversation »** : il impose un dépôt cible et une Pipeline, qu'une question n'a pas.
- **Une Session hébergée** : réservée aux Extensions (ADR-0092), le core n'en ouvre pas.
- **Une réponse en ligne dans la palette** : une seconde surface de réponse à construire, sans
  terminal pour suivre ce que l'agent fait ni pour lui répondre.
