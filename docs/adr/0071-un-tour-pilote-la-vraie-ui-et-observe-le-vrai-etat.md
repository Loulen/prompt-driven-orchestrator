# Un Tour pilote la vraie UI et observe le vrai état

> Statut : accepted (grilling #816), amendé au grilling #909 (§4 : la préparation peut lancer un vrai Run
> de nodes `script` et poser un objet temporaire que le rangement retire), amendé au grilling PDO-20
> (§5 : les objets d'un tour appartiennent à l'identité de tour du navigateur). Vocabulaire : CONTEXT.md
> § « Tours guidés — onboarding ».

## Contexte

Un nouvel utilisateur arrive devant un canvas vide et rien ne le guide vers un premier Run ni une
première pipeline. Il fallait un mode tutoriel. Trois façons de le faire : des captures ou une vidéo,
un mode démo sur des données factices, ou des tours qui guident l'utilisateur dans l'application
réelle.

## Décisions

**1. Un Tour guide dans la vraie UI et n'avance que sur l'état réel observé.** Le projecteur grise
tout sauf la cible, l'instruction dit quoi faire, et l'étape passe quand l'état attendu est atteint
(modale ouverte, valeur choisie, Run lancé, node terminé) — jamais sur un délai. Écartées : les
captures et vidéos (elles rotent à chaque changement d'UI et n'apprennent pas le geste) ; un mode démo
sur données factices (deux chemins de code à maintenir, et l'utilisateur ne repart avec rien).

**2. Pendant les étapes, le tour n'agit jamais à la place de l'utilisateur.** Il observe et bloque ; il
ne remplit pas un champ, ne clique pas, ne lance rien. Skip passe à l'étape suivante et laisse la
validation normale du formulaire faire son travail ; si une action requise manque, l'étape échoue
proprement et dit pourquoi. Corollaire du principe « l'outil n'agit pas de sa propre initiative »
(ADR-0012) et de ce qu'un tour doit enseigner : le geste, pas son résultat. Écarté : un Skip qui
auto-remplit — l'utilisateur finit le tour sans avoir appris où se trouvait le champ.

**3. Le daemon ignore la notion de tour.** Il expose des verbes génériques (créer un dépôt git à
partir de paramètres, créer une pipeline à partir d'un document, créer un Run, créer et supprimer un
Trigger) et c'est le frontend qui compose un tour avec. Les définitions de tours sont des données
déclaratives côté frontend (cible, instruction, condition d'avancement, skippable, préparation,
rangement). Écarté : un endpoint « démarrer le tutoriel » qui préparerait tout en une fois — il
couplerait le daemon à un contenu pédagogique qui changera plus souvent que lui.

**4. La préparation installe un état réel à lire ; le rangement retire ce qui n'appartient pas à
l'utilisateur.** Un tour qui fait *visiter* l'écran (*Overview*) a besoin d'un Run terminé avec des
outputs, un terminal et une pastille, sans harnais configuré ni coût. Sa préparation lance donc un
**vrai Run d'une pipeline de nodes `script`** (ADR-0017) : il se termine en quelques secondes, ses
outputs sont écrits et validés, son worktree existe et s'archive par le vrai geste. La préparation
n'est pas une étape : elle se joue derrière la carte d'intro, avant que l'utilisateur ait quoi que ce
soit à apprendre, et reste idempotente (réutilise dépôt, pipeline, et le dernier Run terminé non
archivé *de la même identité de tour*, §5).
Un objet posé uniquement pour être montré (le Trigger d'exemple) est rendu **inoffensif par
construction** — actif, mais son guard refuse toujours, il ne lance jamais de Run — et le **rangement**
du tour le supprime à toute sortie (fin, quitter, arrêt propre). Ce que l'utilisateur garderait après
*First run* (pipeline, Run) reste à lui.

Écartées : **importer un Run enregistré** (verbe daemon « Run depuis un document ») — les nodes
auraient été de vrais agents, mais sans worktree l'archivage n'aurait rien supprimé, le verbe se
couplait au format de l'event log, et l'état aurait été simulé, ce que §1 refuse ; **pas de Run du
tout**, outputs et terminal désignés comme zones vides — moins cher, mais le tour ne montre rien de ce
qu'il explique. Le prix assumé : le tour dit qu'un node est « en général un agent » en montrant des
scripts.

**5. Ce qu'un tour pose ou fait poser appartient à l'identité de tour du navigateur.** Une instance
PDO se partage (le Dojo en fait jouer les tours à plusieurs en même temps) et PDO n'a aucune notion
d'utilisateur. Un tour reconnaissait ses objets à leur nom fixe ou à « le plus récent » : le Run
d'*Overview* réutilisé par tous (l'archivage de l'un bloquait l'étape d'archivage des autres), le
Trigger d'exemple que le rangement de l'un retirait sous les yeux d'un autre, « ton Run » de *First
run* lu comme le premier de la liste (le Launch d'un voisin validait ton étape), et le nom de
pipeline de *First pipeline* refusé au deuxième joueur. Désormais chaque navigateur tire une fois une
**identité de tour** (4 caractères base36, mémoire navigateur) que tous les noms posés ou collés par un
tour portent en suffixe. La préparation ne réutilise, et le rangement ne retire, que les objets de
cette identité ; le Run lancé par l'utilisateur se reconnaît à l'id que son propre lancement a reçu,
jamais à sa position dans la liste. Ce que les tours ne font que lire (dépôt d'entraînement, pipelines
du tour, Projet *Tutorial*) reste commun. Le daemon ignore toujours les tours (§3) : l'identité est un
suffixe de nom, pas un champ.

Écartées : **un identifiant par passage** — chaque replay empilerait un Run et un Trigger, ce que
l'idempotence de §4 refuse ; **un prénom saisi** au lancement — une étape de plus, et deux « Alex »
se marchent encore dessus ; **une identité côté daemon** (utilisateurs, sessions) — elle couplerait
le daemon aux tours pour un besoin d'onboarding. Le prix assumé : une identité perdue (mémoire vidée,
autre navigateur) en tire une nouvelle et laisse ses anciens objets orphelins ; le Trigger y reste
inoffensif par construction.

## Conséquences

- *First run* dépend d'un harnais configuré pour aller jusqu'au bout ; sans lui, l'étape de lancement
  échoue proprement avec la raison du daemon, ce qui est déjà une leçon. *Overview*, lui, n'en dépend
  pas.
- Les cibles de tour sont des éléments réels de l'UI : un champ dont les options ne sont pas
  ciblables (`<select>` natif) doit être remplacé par le menu maison avant d'entrer dans un tour.
- Le dépôt d'entraînement vit sous `/tmp` et disparaît avec lui ; aucun nettoyage à gérer.
- Un onglet fermé en plein tour saute le rangement : le Trigger d'exemple reste, inoffensif, et le
  prochain passage du même navigateur le reprend par son nom avant de le ranger. Un Trigger d'exemple
  hérité, sans suffixe d'identité, est retiré par le rangement de n'importe quel passage.
- Les Runs des tours sont de vrais Runs : ils comptent dans Stats et dans la règle de la Modale de
  bienvenue.
