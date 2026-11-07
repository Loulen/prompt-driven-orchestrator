# Un geste refusé est grisé, avec une phrase lisible fournie par l'extension qui refuse

> Statut : accepted (CORE-205). Numéro attribué à la fusion vers `main` (ADR-0095).
> Revient sur le masquage des gestes refusés décidé par CORE-42.

## Contexte

CORE-42 masquait les gestes refusés : la vue d'un Run, la liste des Runs, ses actions groupées, la
palette et les Actions d'extension gardées par une Opération ne montraient que ce que la Question
d'autorisation (`POST /authorize`) permettait. D'autres surfaces (Settings, Agent profiles, banque
de skills, Triggers, Review…) laissaient partir l'action et affichaient ensuite le refus brut du
démon : `operation refused: <reason>`, où la raison technique d'une extension parle de ses propres
règles.

Deux conséquences : un Acteur à qui un geste est refusé ne savait pas qu'il existait, ni à qui le
demander ; et un humain lisait le jargon technique de l'extension.

## Décision

1. **Griser au lieu de masquer.** Un geste que l'Acteur ne peut pas faire reste visible et grisé
   (`aria-disabled`, pas `disabled` : il garde le focus et son infobulle), sans effet au clic.
   L'infobulle dit la phrase de refus. Dans un menu, les entrées grisées ne portent pas
   d'infobulle chacune : le menu se termine par une ligne, un cadenas et la phrase (chaque phrase
   distincte une fois). Le kit `@pdo/extension-ui` porte ce rendu : `disabledReason` sur `Button`,
   `IconButton`, une entrée de `Menu` et un choix de `Segmented`.
2. **Interroger avant d'agir.** Une surface dont l'action est gardée par une Opération pose la
   Question d'autorisation à l'ouverture, une fois pour ses gestes, et grise ceux qui sont
   refusés, plutôt que de laisser partir l'action.
3. **Une phrase pour un humain, une raison pour un programme.** Un refus porte deux textes :
   `reason`, la cause technique (journal, diagnostic d'un agent), et `sentence`, ce qu'une
   personne lit : ce qu'elle ne peut pas faire et qui peut le lui accorder, jamais le vocabulaire
   technique de l'extension. L'extension qui refuse fournit la phrase (`Authorization::deny(..).saying(..)`) ;
   sans phrase, le core dit une phrase générique (« You don't have the rights to do this. »).
   `POST /authorize` et le corps `403` `operation_refused` portent les deux.
4. **Chacun lit le sien.** L'UI du core n'affiche que la phrase (infobulle, toast, ligne
   d'erreur), jamais la raison. La CLI affiche la phrase suivie de la cause entre parenthèses, ce
   qu'un agent lit pour diagnostiquer.

## Conséquences

- Sans extension qui restreint, rien n'est grisé : toutes les décisions sont `allow`.
- Un choix dans une liste (les Projets où créer un Run dans New Run) reste filtré : ce n'est pas
  un geste, et la note dit combien de Projets sont cachés et pourquoi.
- `Authorization::Deny` et `Denied` gagnent un champ : un motif `Deny { reason }` devient
  `Deny { reason, .. }`, un littéral `Denied { reason }` devient `Denied::new(reason)`. L'API
  d'extension étant en 0.x, c'est une version mineure, annoncée comme cassante.
- Le démon refuse toujours ce que l'UI montre actif : la Question d'autorisation éclaire l'UI,
  elle ne remplace pas l'Autorisation d'opération.

## Alternatives écartées

- **Garder le masquage (CORE-42)** : l'Acteur ignore que le geste existe et qui peut l'accorder.
- **Une infobulle par entrée de menu** : répétitive quand toutes les entrées sont refusées pour la
  même raison ; une ligne en pied de menu se lit une fois (option B du design).
- **Traduire la raison dans le core** : le core ne connaît pas les règles d'une extension ; seule
  l'extension qui refuse sait dire qui peut accorder le geste.
