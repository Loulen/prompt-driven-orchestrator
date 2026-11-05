# Start est l'unique point d'entrée ; un Pipeline non lançable est refusé au lancement

> Statut : accepted (grilling du 2026-11-05, issue gitlab `pdo-issues#23` « Un pipeline dont Start
> n'est pas relié à End devrait être signalé »). **Amende ADR-0001** : la cohérence de chemin est un
> diagnostic de correction, et elle bloque le lancement (jamais l'enregistrement). Vocabulaire :
> CONTEXT.md « Pipeline non lançable », « Cul-de-sac », « Nœud orphelin ».

## Contexte

Le scheduler est né (#4) avant les nœuds `Start`/`End` : un nœud sans arête entrante était alors
*le* point d'entrée d'un Run. Quand `Start`/`End` sont devenus mandatoires (#39), le scheduler n'a
pas été réécrit : une arête depuis `Start` est simplement ignorée dans le calcul des amonts. Un nœud
relié à `Start` et un nœud relié à rien démarrent donc **à l'identique**, au premier tick, et
`Start` ne sert qu'à transmettre `user_prompt`. Résultat : on enregistre et on lance des pipelines
sans chemin de `Start` à `End` (le tuto *First pipeline* en produit un à chaque passage), qui
tournent sans prompt ou n'aboutissent jamais, sans que rien ne le signale.

## Décision

Un Pipeline est **lançable** si et seulement si, en suivant **toutes** les arêtes (conditionnelles
comprises, ré-entrées de boucle comprises — la cohérence est statique, les prédicats restent
évalués au runtime, ADR-0002) :

1. `End` est atteignable depuis `Start` ;
2. **aucun Cul-de-sac** : tout nœud, sauf `End`, mène à `End` ;
3. **aucun Nœud orphelin** : tout nœud est atteignable depuis `Start`.

`Start` devient ainsi **l'unique point d'entrée** d'un Pipeline — sinon autant le supprimer.

- **Lancement** : tout chemin qui crée un Run (New Run, `pdo run create`, Trigger, Retry all, Run
  enfant) refuse un Pipeline non lançable, en nommant chaque défaut et chaque nœud fautif. Un
  Trigger branché sur un Pipeline non lançable est *dangling* et ne tente pas de firer.
- **Édition** : jamais bloquée. Les défauts s'affichent comme diagnostic de correction (toujours
  visible, sans croix), en direct pendant l'édition et à l'enregistrement ; New Run grise le
  Pipeline. Le daemon reste la seule autorité ; le contrôle du front n'est qu'un miroir.
- **Le scheduler n'est pas modifié** : le refus au lancement garantit qu'aucun nouveau Run ne
  contient d'orphelin. Les Runs déjà lancés et le retry d'un nœud dans un Run ne sont pas concernés.

## Pourquoi, contre ADR-0001

ADR-0001 refuse de valider la *qualité* d'un graphe. Un Pipeline non lançable n'est pas un design
exotique : il ne peut rien produire (pas de chemin vers `End`), ou il fait tourner des nœuds sans
le prompt du Run et en dehors de tout chemin vers `End` — une altération **silencieuse** du sens du
Pipeline, exactement ce que la famille « diagnostic de correction » (#268) couvre déjà pour les
arêtes dangling, elles aussi refusées au lancement. L'enregistrement reste libre : on peut
toujours dessiner un graphe inachevé.

## Options écartées

- **Orphelin = simple avertissement.** Laisse vivre deux sémantiques d'entrée (`Start` et « nœud
  sans entrée ») pour un même graphe ; `Start` n'aurait alors aucun rôle d'orchestration.
- **Ne démarrer au runtime que les nœuds atteints depuis `Start`.** Un orphelin qui alimente un
  nœud atteint depuis `Start` ferait attendre ce nœud indéfiniment : un stall silencieux, interdit
  (ADR-0011). Le refus au lancement obtient le même invariant sans toucher au scheduler.
- **Bloquer l'enregistrement.** Contraire à ADR-0001 : un graphe en cours de dessin est
  légitimement incomplet.

## Mesure

Sur les pipelines de la machine de dev au 2026-11-05 : aucun pipeline réel (planner, review-loop,
grill-orchestrate, implement-loop, conversation…) n'est rendu non lançable ; seules 10 copies issues
du tuto *First pipeline* le sont, toutes faute d'arête depuis `Start`.
