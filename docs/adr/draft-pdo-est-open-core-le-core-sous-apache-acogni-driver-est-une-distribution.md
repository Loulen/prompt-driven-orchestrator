# PDO est open core : le core sous Apache 2.0, Acogni Driver est une Distribution aux extensions privées

Le core (le binaire vanilla `pdo`, son front, la crate `pdo-extension-api`) est publié sous le nom **PDO**, licence **Apache 2.0**, dans le dépôt public `Loulen/prompt-driven-orchestrator` qui remplace la v1 sur place. La version entreprise, **Acogni Driver**, est la Distribution (ADR-0081) qui compile ce core avec des extensions privées (droits, gestion de projet…) ; elle n'est jamais un fork. Acogni, la structure qui publie les deux, garde dans le dépôt du core son logo et son Thème : Apache 2.0 ne donne aucun droit sur la marque (section 6), le `NOTICE` le rappelle.

Apache plutôt que MIT pour la clause de brevet et le `NOTICE`, qui portent la marque Acogni et la notice MIT des contributions antérieures (le MIT autorise la relicence). Les versions jusqu'à 1.132.0 restent sous MIT.

## Considered Options

- **Fork entreprise** : deux bases de code à tenir à jour pour un mainteneur seul ; l'API d'extension existe justement pour éviter ça.
- **Core en partie fermé** : la ligne « tout le core est ouvert, la valeur entreprise est dans les extensions » est la seule qu'un contributeur peut lire sans consulter le mainteneur.
- **Ouvrir certaines extensions dès maintenant** : écarté pour l'instant ; ouvrir une extension plus tard reste possible, l'inverse non.

## Consequences

- L'état disque de la v1 est relu tel quel par la v2 (même `~/.pdo`, même base, même unité de service) : la v2 la remplace par `brew upgrade pdo`, sans migration. Le retour à la v1 n'est pas garanti.
- La cadence de publication d'Acogni Driver n'est pas fixée : la Distribution épingle un tag du core, sans politique pour l'instant.
