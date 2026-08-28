# Les profils agentiques restent des références vivantes jusqu'au spawn

Un profil agentique est un réglage d'instance qui associe un harnais obligatoire à un modèle et un effort facultatifs. Chaque tier choisit Inherit, un profil nommé, ou Custom ; ce dernier porte une combinaison inline complète plutôt que de réactiver l'ancien résolveur. Le premier tier explicite de la chaîne `Node → Run → Projet → Configuration d'instance → Default` fournit atomiquement harnais, modèle et effort. Copier les valeurs d'un profil nommé aurait réduit celui-ci à un raccourci de saisie et empêché une modification centralisée ; sa référence reste donc vivante jusqu'au spawn, puis la combinaison résolue est gelée pour le NodeRun.

Un profil porte un identifiant stable distinct de son nom, donc le renommage préserve ses référents. Le pipeline distingue les nodes en mode Custom de ceux qui suivent un profil afin qu'une évolution du profil ne semble pas ignorée.

L'instance possède un profil à l'identité réservée, initialement nommé `Default`, qui vaut `claude` sans modèle ni effort. Il reste modifiable et renommable, mais ne peut pas être supprimé. Après suppression d'un autre profil, ses référents affichent un avertissement puis reprennent la précédence au tier suivant ; le picker montre sous chaque nom la combinaison qu'il résout.

Les noms sont uniques sans distinction de casse. Une suppression confirmée après affichage des pipelines, nodes, Runs non démarrés, Projets et réglages d'instance concernés ne touche jamais un NodeRun déjà démarré. Lorsqu'un Run choisit une combinaison, ses sessions d'infrastructure reçoivent aussi son modèle et son effort, afin que le profil décrive réellement toutes les sessions du Run.

Une édition ne salit pas les pipelines référents : ils stockent l'identifiant et affichent la combinaison vivante. Au spawn, PDO lit et gèle une seule révision complète du profil afin qu'une écriture concurrente ne puisse jamais mélanger deux combinaisons. Une référence absente apparaît dans l'avertissement global du pipeline et dans son picker, puis la résolution reprend au tier suivant.
