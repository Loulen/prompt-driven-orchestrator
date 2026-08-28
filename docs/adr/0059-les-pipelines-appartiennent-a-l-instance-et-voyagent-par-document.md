# Les pipelines appartiennent à l'instance et voyagent par document

Une Pipeline est une ressource de l'instance PDO, et non une ressource `repo`, `user` ou `library`. Cette propriété unique supprime des comportements artificiellement différents ; le partage entre instances ou dépôts devient une action explicite au moyen d'un Document de pipeline transportable, interprété vers le stockage interne et fidèle à tout le contenu possédé par la Pipeline.

Le document développe les nodes partagés mais n'embarque aucune configuration d'instance, valeur d'exécution, variable d'environnement ou secret. Cette limite rend l'échange sûr et indépendant des installations externes, au prix de ramener à **Inherit** les choix qui dépendaient d'un profil agentique nommé.
