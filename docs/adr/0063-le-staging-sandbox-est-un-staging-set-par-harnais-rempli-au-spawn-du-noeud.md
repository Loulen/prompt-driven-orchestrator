# ADR-0063 — Le staging sandbox est un *staging set* par harnais, rempli au spawn du nœud qui le résout ; le profil reste un diff harnais-agnostique

Sans cet ADR, un agent accueillerait un troisième harnais dans le sandbox soit par une variante de
plus dans le « plancher » de claude (`StagingFloor::PiAgentDir`, ce que proposait #702), soit par
une recette « écris un profil qui copie `~/.pi/agent` », soit par un Dockerfile de plus dans le dépôt.
Les trois font porter à l'utilisateur ou au code de claude ce qui appartient au harnais.

> Statut : accepted (grilling du 2026-09-05, story #702 « pi, troisième harnais first-party »).
> Vocabulaire : CONTEXT.md § *Sandbox*. **Amende ADR-0031 §1** : le « plancher » profil-agnostique
> devient le *staging set* de `claude`, un cas parmi d'autres. **Amende ADR-0045** (limites) : « le
> sandbox est hors périmètre, le plancher reste propre à `claude` » cesse d'être vrai. **Amende
> ADR-0051** : la capacité de staging reste un point de dispatch, mais ce qu'elle rend est une liste,
> pas cinq gestes.

## Ce qu'on décide

1. **Chaque harnais first-party déclare un *staging set*** : les entrées de `$HOME` et les variables
   d'env qui font qu'une session dans le conteneur se comporte comme sur l'hôte — authentification,
   réglages, catalogue de modèles, extensions, skills. Une entrée peut être marquée **transcripts** :
   exclue à l'aller, **rapatriée** au merge-back (`.claude/projects/`, `.pi/agent/sessions/`). Le
   staging set est du code, comme les autres capacités (ADR-0045) ; `None` est une valeur explicite
   (ADR-0051), publiée dans le tableau de support.
2. **Les *autonomy fixups* sont une seconde chose, distincte** : les écritures qui désarment un
   dialogue bloquant une fois le set copié. `claude` en a (confiance, bypass permissions, managed
   settings consentis) ; `pi` et `copilot` n'en ont pas, leur argv porte le flag (`-a`,
   `--allow-all`). Les cinq garanties d'ADR-0031 §1 restent, mais elles sont *celles de claude*.
3. **Le staging set se remplit au spawn du nœud qui résout le harnais, pas au `RunStarted`.** Le
   conteneur est créé une fois par Run et ses mounts sont figés à ce moment ; le harnais d'un nœud,
   lui, n'est connu qu'au spawn (ADR-0046). PDO monte donc **vide**, à la création, la racine de home
   de chaque harnais first-party, puis copie le set d'un harnais **la première fois qu'un nœud du Run
   le résout**, jamais avant. Un Run qui ne lance que `claude` n'a jamais l'auth de `pi` dans son
   conteneur. Un Run qui épingle les deux les a tous deux, une fois chacun.
4. **Le profil de staging reste le diff de l'utilisateur, harnais-agnostique** : entrées en plus,
   env en plus, image. Il ne sait pas ce qu'est un harnais et n'a pas à le savoir.
5. **L'image n'est pas l'affaire de PDO.** PDO ne fournit pas de Dockerfile par harnais ; il vérifie
   au spawn que le binaire résolu existe dans le conteneur et, sinon, le dit une fois et laisse le
   nœud `Interrupted` avec cette raison (ADR-0049).

## Pourquoi, et ce qui a tué les alternatives

**« Un profil par harnais » est le mauvais axe.** Le profil est **par Run** et un Run épingle
plusieurs harnais (HP-02 étape 14 en lance trois). Un profil `pi` n'aurait de sens que pour un Run
mono-harnais, et l'utilisateur devrait composer à la main le produit cartésien des combinaisons.

**Copier « tout harnais installé sur l'hôte » au `RunStarted` a été écarté** malgré sa simplicité :
il donne à une sandbox qui ne fait tourner que `claude` les moyens de lancer `pi` avec les
credentials de l'utilisateur. Le remplissage au spawn coûte quelques répertoires vides montés en
plus et rend le périmètre exact : ce qui est dans le conteneur est ce que le Run a résolu.

**Calculer le superset des harnais résolubles au `RunStarted`** (pins des nœuds + tiers Run,
Projet, instance) a été écarté aussi : le graphe s'édite pendant un Run (CONTEXT.md § *Édition
pendant un Run*), et un nœud ré-épinglé après le départ trouverait un home vide. Le remplissage au
spawn suit la résolution réelle, donc l'édition à chaud.

**Une recette documentée** (« ajoute `.pi/agent/auth.json` à ton profil ») a été écartée : c'est
exactement la connaissance que PDO possède déjà pour `claude` et qu'il synthétise plutôt que
d'exiger. Mesuré au grilling : les profils d'ADR-0031 acceptent déjà toute entrée `$HOME`-relative
plus un `env`, donc la recette *marchait* — elle déplaçait juste vers l'utilisateur ce qu'un harnais
first-party doit savoir sur lui-même.

**Un Dockerfile par harnais dans le dépôt** a été écarté : l'image est l'environnement de
l'utilisateur, pas une propriété du harnais, et le Dockerfile seedé lui-même a vocation à quitter ce
dépôt.

## Limites acceptées

- **Un harnais déclaré sur disque n'a pas de staging set** (pas de code) : dans un Run sandboxé il
  ne tient que par le profil de l'utilisateur, comme avant.
- **Le rapatriement des transcripts suppose la parité de chemin** : `pi` nomme le répertoire de
  sessions d'après le cwd ; le worktree est monté au même chemin absolu dans le conteneur
  (ADR-0030), donc le nom est identique dedans et dehors. Un futur remontage ailleurs casserait la
  résolution de transcript des Runs sandboxés — c'est un invariant à tenir, pas un détail.
- **La vérification du binaire dans l'image est un `which` au spawn**, pas une validation d'image :
  un binaire présent mais d'une version non validée passe, comme sur l'hôte (les versions du tableau
  de support sont des bornes documentées, pas des gardes).
