# ADR-0046 — Le harnais agentique est un axe à quatre tiers ; le modèle et l'effort sont conditionnés par lui, jamais résolus à part

> Statut : accepted (grilling du 2026-08-14). Vocabulaire : CONTEXT.md §*Harnais agentique*, §*Modèle
> et effort*, §*Projet*. **Amende ADR-0015** (la Configuration d'instance gagne un tier au-dessus
> d'elle, et son défaut de modèle devient un défaut *par harnais*). Forme du descripteur et contrat
> d'éligibilité d'un harnais → **ADR-0045**.
>
> **Amendé par ADR-0053** : le modèle et l'effort restent conditionnés par le harnais, mais ce qui est
> *offert* pour un harnais est **déduit du binaire installé** et publié par le daemon, jamais écrit en
> dur — ni côté daemon, ni côté client.

## Contexte

PDO n'a jamais lancé qu'un seul harnais, `claude`. Le modèle est donc un axe autonome à deux tiers
(`node` → instance → défaut du compte, #296/#347) et l'effort un troisième axe sans défaut (#424).
Ouvrir le produit à plusieurs harnais casse ce découpage : un identifiant de modèle n'a pas
d'existence hors du harnais qui l'accepte.

## Ce qu'on décide

1. **Le harnais est un axe à quatre tiers** : `node` → Run → Projet → Configuration d'instance →
   plancher (`claude`). Il se résout **une fois, au spawn**, et se gèle dans l'événement de démarrage
   du nœud.
2. **Le modèle et l'effort ne sont pas des axes.** Un node porte une **carte par harnais** ; les
   valeurs se lisent dans l'entrée du **harnais gagnant**. Aucune précédence propre, aucun merge de
   champs entre tiers de harnais différents.
3. **Le tier intermédiaire est un Projet** — un regroupement nommé de dépôts membres — et non le
   repo cible nu.
4. **Les sessions d'infra** (Pipeline Manager, résolveur de merge) suivent le harnais **du Run**.

## Pourquoi, et ce qui a tué les alternatives

**Garder le modèle comme axe séparé est réfuté par le tier Run.** Ce tier existe pour relancer le
*même* pipeline sur un autre harnais. Un node qui porte un modèle est plus fin que le Run, donc il
gagne : le Run bascule sur `opencode` et chaque nœud est lancé avec un slug Anthropic, donc **tous**
échouent au démarrage. Aucun réaménagement de précédence ne sauve ce cas — ce n'est pas un conflit de
tiers, c'est une valeur qui n'a pas de sens dans l'autre harnais. La carte est la seule forme où un
changement de harnais à un tier grossier reste exécutable.

**Le triple atomique par tier** (le tier gagnant fournit harnais + modèle + effort d'un bloc) est
écarté par ses effets de bord : passer un node en modèle capable forcerait à re-nommer son harnais, et
un réglage posé sur un Projet écraserait l'effort de tous ses nœuds.

**Le tier intermédiaire ne peut pas être le repo cible.** Le repo cible est stocké **verbatim, jamais
canonicalisé** (ADR-0033) et les listes le comparent verbatim : un store keyé dessus donnerait deux
configurations pour deux orthographes du même chemin, sans réconciliation possible. Et depuis
ADR-0042 un Run porte plusieurs dépôts — « le dépôt » n'est plus une clé. Le Projet nomme le
regroupement une fois ; l'appartenance, elle, reste une comparaison verbatim.

**L'identité d'un Projet n'est pas l'ensemble des dépôts d'un Run.** Le multi-repo étant un axe
par-Run (ADR-0042), cet ensemble varie d'un Run au suivant : en faire l'identité ferait disparaître le
nom donné *et* le réglage attaché dès qu'un Run ajoute un dépôt secondaire.

**Un Projet est matérialisé à la demande, jamais seedé.** Le groupement des listes est déjà dérivé du
chemin ; une ligne n'existe que si un humain nomme le groupe ou y attache un réglage. Même posture que
la table de prix (ADR-0034 : rien n'est seedé) et que les agrégats (ADR-0029 : dérivé à la lecture).

**Le gel au spawn** reprend la leçon de #424 : la reprise doit re-poser ce qui a été **lancé**, pas ce
que le YAML ou le Projet disent maintenant — une édition n'atteint jamais l'itération vivante d'un
nœud (ADR-0007).

## Limites acceptées

- **Un node sans entrée pour le harnais gagnant tourne sans modèle**, donc au défaut du compte de ce
  harnais. C'est exactement ce que « pas de modèle posé » veut déjà dire ; on n'invente pas un refus.
- **Les sessions d'infra suivant le Run**, un A/B sur un harnais neuf met aussi le manager à
  l'épreuve : l'outil de déblocage est celui qu'on teste. Choisi en connaissance de cause, pour que
  « tout ce Run tourne sur X » reste vrai sans exception à retenir.
- **Le YAML change de forme** : le champ de modèle plat devient une entrée de carte sous `claude`.
  Breaking, annoncé en semver, migré par le migrateur de pipelines.
- **Quatre tiers, c'est un de plus que le sandbox** (`Run → Trigger → instance → off`). Le tier
  Trigger n'est pas dupliqué ici : le template d'un Trigger *est* la charge utile d'un `POST /runs`,
  donc un Trigger pose son harnais par construction.
