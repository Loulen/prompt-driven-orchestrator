# Les médias du README sont joués par de vrais agents sur une instance de démo isolée

> Statut : accepted (grilling #852), amendé au 2ᵉ grilling de #852 (§6). Vocabulaire : CONTEXT.md § « Médias du README ».

## Contexte

Le README devient une vitrine (modèle : le README d'Orca) : un hero animé et un tableau de features
dont chaque ligne porte un GIF. Ces GIF doivent suivre l'UI sans capture manuelle, montrer des gestes
(glisser une arête, poser une condition, envoyer un commentaire au manager) et des écrans pleins
(Stats par modèle, arbre de runs), sans polluer l'instance de l'utilisateur ni coûter un budget
d'agents à chaque régénération.

## Décisions

**1. Un script reproductible pilote la vraie UI d'une instance de démo.** `make readme-media`
démarre un daemon isolé, pilote l'UI au navigateur, enregistre, puis monte chaque **Scène** en GIF +
poster. Écartée : la capture manuelle, qui rote au premier changement d'UI et ne se refait pas à
l'identique.

**2. Les scènes live sont jouées par de vrais agents, arrêtés dès l'enregistrement fini.** Le hero,
la review de diff et l'arbre de runs montrent `claude` sur `claude-opus-5-5` qui travaille vraiment
sur un dépôt fixture versionné. Écarté : un faux agent qui rejoue une session scriptée — moins cher
et déterministe, mais ce que le README montre ne serait plus le produit.

**3. L'historique est moqué, jamais produit en direct.** Stats par modèle, fires de trigger et runs
terminés de la liste sont insérés dans l'event log et accompagnés de transcripts synthétiques
(environ 50 exécutions par couple modèle × nœud). Un Stats plein demande des dizaines d'exécutions
réparties sur des semaines : impossible en temps réel, et hors de prix. Les ordres de grandeur visés
(coûts relatifs, taux d'échec) sont une donnée de la démo, pas une mesure.

**4. L'instance de démo est étanche : son propre répertoire courant, son propre `HOME`, son propre
port.** La base suit le répertoire courant du daemon ; pipelines, banque de skills, profils, tables
de prix et transcripts suivent `HOME` ; le socket tmux suit le port. Un `HOME` partagé ferait tomber
les profils créés, les skills importés et les transcripts moqués dans l'instance réelle. Conséquence
assumée : les fichiers d'authentification des harnais joués en direct sont copiés dans le `HOME` de
démo le temps de l'enregistrement, puis effacés.

**5. Deux variantes par scène, une sélection versionnée.** Chaque scène produit deux variantes dans
un dossier de revue non versionné ; un fichier de sélection versionné désigne celle qui devient le
média publié, et seules les variantes retenues sont commitées. Pas de budget de poids : les GIF sont
commités dans le dépôt comme chez Orca, on reverra si le poids gêne.

**6. Les pipelines des scènes sont dessinés par le mainteneur et installés tels quels** (amendement,
2ᵉ grilling de #852). Le premier jet générait la mise en page d'un pipeline unique : au test, les
positions et les tracés d'arêtes donnaient l'image d'une app brouillonne, alors que le contenu
(prompts, nœuds, outputs) était bon. Le jugement visuel est humain, il revient donc au mainteneur :
il dessine chaque **Pipeline cible** dans l'éditeur PDO (modèle d'arêtes de #840 : ancres, tracés,
labels), et l'outil l'installe à l'octet près dans l'instance de démo (seul son `name:` est ramené au nom de démo). Trois cibles :
`implement-review` à l'état « construit » (Visual pipelines), `implement-review` complet avec sa
boucle (Routing & loops, le hero et toutes les scènes live), et `prod-check`, le pipeline déclenché
par le trigger. La règle « un seul pipeline » tombe ; la continuité reste, car le visiteur voit le
même `implement-review` se construire, boucler puis tourner. Une scène qui construit à l'écran part
de la cible privée de ce qu'elle dessine, vise les coordonnées de la cible, et réinstalle la cible
hors caméra avant son plan final : le poster est exactement le dessin. Écartées : une mise en page
générée ou retouchée par l'outil (c'est elle qui a échoué), et une greffe à chaque import de la mise
en page du mainteneur sur le contenu versionné (appariement fragile, pour un dessin qui change
rarement). Le contenu et la mise en page ont été fusionnés **une fois**, validés à l'écran, et le
fichier entier fait foi depuis.

## Conséquences

- Régénérer les médias coûte une session d'agents réelle : on ne le fait que quand une scène change
  visiblement.
- Chaque régénération ajoute ses binaires à l'historique git ; c'est accepté.
- Un changement de format de l'event log ou des transcripts casse le seed de l'historique moqué :
  le seed réutilise la recette des tests de Stats par modèle, qui cassent en même temps.
- Redessiner une scène passe par l'éditeur PDO puis un import dans la fixture ; un prompt modifié
  dans l'éditeur part avec le fichier.
- Les cibles suivent le modèle d'arêtes du produit : un changement de ce modèle impose de les
  redessiner ou de les migrer.
