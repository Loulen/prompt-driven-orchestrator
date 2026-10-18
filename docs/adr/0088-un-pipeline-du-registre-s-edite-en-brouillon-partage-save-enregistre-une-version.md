# Un Pipeline du registre s'édite en brouillon partagé ; Save enregistre une version

> Statut : accepted (grilling du 2026-10-18, CORE-77). Vocabulaire : CONTEXT.md § *Persistance,
> brouillon partagé et versions*. Amende ADR-0048/ADR-0051 (où écrit l'assistant de bibliothèque).

## Contexte

Le canvas était le fichier YAML, enregistré par un Save explicite, et une écriture externe du
fichier l'emportait (last-write-wins). À plusieurs, une personne ne voyait les changements d'une
autre qu'après son Save, sous forme de fichier entier, et deux éditions simultanées finissaient
dans une fenêtre de conflit. Il n'existait aucun historique : Save écrasait le fichier.

## Décision

1. **Un brouillon partagé par Pipeline du registre**, tenu par le daemon et conservé à travers un
   redémarrage. Chaque modification (nœud, edge, champ, prompt) s'y applique et se diffuse
   aussitôt, fusionnée sans conflit au grain du champ (structure de données répliquée sans
   conflit), avec la présence des participants.
2. **Save fige le brouillon en version** (numéro croissant, titre `vN` par défaut, modifiable), ou
   remplace la version courante, ou garde la version enregistrée comme Pipeline distinct. Le
   fichier du registre est toujours **la dernière version** : c'est elle que lancent New Run et les
   Triggers, et elle qu'on exporte. Le brouillon n'est jamais lancé ; un Run retient sa version.
3. **Lancer depuis l'éditeur** un brouillon qui diffère de la dernière version demande
   explicitement d'enregistrer une version ou de lancer la dernière : plus d'enregistrement
   silencieux au lancement.
4. **L'assistant de bibliothèque écrit dans le brouillon**, comme un participant signé de son
   Acteur, et plus dans le fichier.
5. **Une écriture externe du fichier** devient une version « external edit » et se fusionne dans
   le brouillon au grain du champ. Pas de verrou de fichier.
6. **Undo n'annule que ses propres changements.**
7. L'édition « pour ce Run » (snapshot run-scope, ADR-0085) reste hors du brouillon partagé.

## Alternatives écartées

- **Sauvegarde automatique et diffusion, dernière écriture gagnante** : supprime l'attente du
  Save mais pas les écrasements entre deux personnes sur le même champ, et rend une version
  impossible à distinguer d'une frappe.
- **Fusion au Save** : garde le modèle actuel et ne fait que repousser le conflit.
- **Verrouiller le fichier** contre les écritures externes : un système de fichiers n'offre qu'un
  verrou contournable (`:w!`, remplacement du fichier) ; fusionner ne perd rien et ne demande
  aucun contournement.
