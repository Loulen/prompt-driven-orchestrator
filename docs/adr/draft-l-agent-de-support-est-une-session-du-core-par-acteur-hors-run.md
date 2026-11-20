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
   confirmation avant toute mutation, où chercher), puis la première question. Sa documentation
   est la CLI du core et des Extensions (`pdo help`, `pdo ext <id> help`) : aucune base
   documentaire n'est rédigée pour lui.
5. **Il travaille dans un dossier par Acteur sous le répertoire d'état de PDO**
   (`~/.pdo/support/<session>/`), que le core prépare à l'ouverture. Le balayage ne le tue pas :
   il s'arrête quand son Acteur le ferme.

## Conséquences

- La palette (Ctrl+K) a un groupe « Support » : ouvrir l'agent, lui poser la question tapée,
  interroger l'agent ouvert sans fenêtre, en ouvrir un autre. L'agent s'affiche dans le Terminal
  agrandi, avec « End session ».
- L'API d'Extension gagne `SessionKind::SupportAgent` : une Extension qui contribue aux sessions
  d'agent le voit passer, avec l'Acteur qui l'ouvre.
- Une session que personne ne ferme reste vivante ; le délai d'inactivité est l'affaire d'un
  ticket suivant de la story.

## Alternatives écartées

- **Un Run « conversation »** : il impose un dépôt cible et une Pipeline, qu'une question n'a pas.
- **Une Session hébergée** : réservée aux Extensions (ADR-0092), le core n'en ouvre pas.
- **Une réponse en ligne dans la palette** : une seconde surface de réponse à construire, sans
  terminal pour suivre ce que l'agent fait ni pour lui répondre.
