# ADR-0033 — Le cwd du daemon n'est jamais une cible de Run implicite : `target_repo` obligatoire à l'écriture, replié à la lecture

Sans cette ADR, on « symétriserait » le champ `target_repo` — soit en gardant le repli sur le cwd du
daemon à l'écriture (le daemon invente alors la seule réponse qui compte : quel dépôt ce travail va
muter), soit en rendant le repli de **lecture** faillible, ce qui ferait disparaître la moitié de
l'historique. L'asymétrie est la conception, et c'est la seule partie qu'un relecteur voudra corriger.

> Statut : accepted (#470). Ferme une fuite d'**ADR-0012(a)** (le runtime n'initie aucun effet durable —
> choisir le dépôt qu'un Run mute en est un) et rend explicite le corollaire d'**ADR-0012(b)** (un
> Trigger est un template de Run : ce qui est obligatoire dans un `POST /runs` l'est dans un Trigger).
> Qualifie la portée de la phrase `WorkingDirectory=` load-bearing d'**ADR-0019** (elle gouverne la
> racine de **stockage**, plus jamais la cible d'un Run). Ne touche pas au repli **de lecture** dont
> dépendent **ADR-0020** (chemins d'archive) et **ADR-0029** (axe « par projet » du coût). Donne enfin
> un ADR à citer aux renvois « même principe que #470 » d'**ADR-0015** et d'**ADR-0031**, qui citaient
> un numéro d'issue faute d'ADR.

## Contexte

Un `POST /runs` sans `target_repo` **réussissait** et créait son worktree dans le dépôt d'où le daemon
avait été lancé — `~/.pdo/app` en production, un dépôt que personne n'a jamais nommé. Le 2026-07-29,
deux Runs y ont écrit du code, récupérable seulement par un `git fetch ~/.pdo/app` que personne
n'aurait pensé à taper.

Ce n'était pas une valeur par défaut mal choisie, c'était **une décision prise à la place de
l'utilisateur sur la seule question qui compte** : quel dépôt ce travail va-t-il muter. Le daemon
n'avait rien à sauver et rien à interpréter ; il a inventé une réponse.

Le point délicat est que le même champ a deux vies. À l'**écriture** il y a un appelant à qui répondre,
et un `null` est une omission. À la **lecture** il n'y a qu'un enregistrement passé à interpréter, et
un `null` est un fait historique légitime : ≈ 46 des 101 Runs de dev n'ont pas de `target_repo`, parce
qu'ils ont été créés quand la frontière était molle. Rendre la lecture faillible pour « symétriser »
ferait disparaître la moitié de l'historique des archives, du coût et des balayages de liveness.

## Ce qu'on décide

### 1. `target_repo` est obligatoire aux quatre frontières d'écriture

Un unique prédicat, `required_target_repo(Option<&str>) -> Result<PathBuf, String>`, porte la règle
pour ses quatre appelants : `create_run_inner`, `create_trigger`, `patch_trigger` et
`POST /triggers/guard/test`. Absent, vide ou blanc ⇒ **400 nommant le champ** et disant quoi passer.
Une seule source, zéro dérive possible (patron `event_log.rs`, leçon #373).

Le prédicat **trim et adopte la valeur trimée** : un seul chemin canonique circule ensuite (validation,
worktree, payload persisté), donc rien ne peut diverger entre ce qu'on valide et ce qu'on stocke.
`" /repo "` est accepté et stocké `"/repo"`.

Le contrôle est le **premier** de `create_run_inner` : aucun `run_id`, aucun event, aucun worktree,
aucune session n'existe encore quand il refuse. C'est ce qui rend « 400 ⇒ rien ne s'est passé » vrai
mécaniquement plutôt que par relecture.

### 2. Le refus d'un Trigger remonte AVANT le guard, pas au chokepoint de création

Sur le chemin de tir, le guard s'exécute *avant* la création du Run. Un 400 au chokepoint arriverait
donc trop tard : un Trigger à dépôt nul aurait déjà lancé son `sh -c` dans le dépôt non nommé — et
5 des 9 Triggers vivants font `git pull` ou `gh issue list`, c'est-à-dire de vrais effets de bord.

Le refus vit donc dans `trigger_dangling_reason`. Trois choses qu'un 400 au chokepoint ne peut pas
acheter tombent ensemble :

- **le guard n'est jamais lancé** — zéro effet de bord dans un dépôt que personne n'a nommé ;
- la transition `Dangling` met `next_fire_at` à `NULL`, donc le Trigger devient **dormant** au lieu
  d'émettre une ligne rouge par tick, indéfiniment ;
- `POST /triggers/:id/fire` répond **409** avec la raison, au lieu d'un `200 {fired:false}` que
  l'opérateur devrait aller déterrer de l'historique des fires.

`trigger_guard_cwd` **disparaît** plutôt que de devenir faillible : la garde amont a déjà prouvé que le
dépôt est présent et valide, donc le compilateur retire un chemin mort.

### 3. Le dry-run de guard obéit à la même règle

`POST /triggers/guard/test` *exécute* un `sh -c` arbitraire, atteignable depuis le bouton « Test
guard ». Son invariant « zéro effet de bord » porte sur le fait de ne pas créer de Run — il ne dit rien
de ce que la commande fait. Le laisser se replier sur le cwd du daemon livrerait un invariant faux pour
un endpoint, donc il refuse aussi.

### 4. PATCH reste un merge partiel : absent ⇒ inchangé, présent-vide ⇒ 400

L'énoncé « un PATCH de Trigger sans dépôt cible ⇒ 400 », lu au pied de la lettre, casse le toggle
enabled/disabled de la liste, qui envoie légitimement `{"enabled": true}`. La règle correcte est :

| corps | effet |
| --- | --- |
| champ **absent** | valeur stockée **inchangée** |
| `"target_repo": null` | **400** |
| `"target_repo": ""` / `"   "` | **400** |
| `"target_repo": "/abs/git/repo"` | validé, stocké trimé |

Câbler `deserialize_double_option` sur `target_repo` sert précisément à rendre le clear
*atteignable* — pour le refuser. Avant, un `null` explicite s'effondrait en `None` côté serde : le
`NewRunModal` en envoyait un à chaque sauvegarde de Trigger et c'était un **no-op silencieux**, donc
vider le champ dépôt dans l'UI *paraissait* marcher et ne faisait rien. Le durcissement ferme ce bug
latent au passage.

### 5. `retry_all` lit le dépôt RÉSOLU, jamais le champ brut

Hors périmètre de l'issue, non négociable. `retry_all` **archive l'original avant** de créer le
remplaçant. En recopiant `target_repo` brut, tout Run antérieur au durcissement (`None`) produirait un
400 **après** l'archivage : original archivé, remplaçant jamais créé, travail injoignable. Le retry lit
donc `effective_repo_root`, atterrit là où l'original a réellement tourné, et l'inscrit explicitement.

### 6. Le repli **de lecture** est conservé, et permanent

`effective_repo_root`, la copie inline de la règle dans `stats.rs` (`cost_project_root`) et
l'`effective_repo` des endpoints de liste gardent leur substitution par `state.repo_root`, sans
changement de comportement. Tous leurs appelants sont des **lectures** : détail de Run, coût
(ADR-0022/0029), chemins d'archive (ADR-0020), balayages de liveness, teardown de worktree.

**L'asymétrie EST la conception** : `target_repo` est obligatoire là où il existe un appelant à qui
répondre 400, et résolu là où il n'y a qu'un enregistrement passé à interpréter. C'est la seule partie
de cet ADR qu'un relecteur voudra « corriger » ; c'est pour ça qu'elle est écrite ici.

## Alternatives écartées

1. **Garder le repli et se contenter d'un `warn!`** — le remède retenu par ADR-0015 et ADR-0031 pour
   leurs propres silences. Écarté parce que ces cas-là préservent une décision que l'utilisateur *a
   prise* et qui a cessé de compter : il y a quelque chose à sauver. Ici il n'a **rien** posé et le
   daemon **inventerait** une cible ; un warn produirait un Run ayant déjà créé sa branche dans un
   dépôt que personne n'a nommé, dans une ligne de log que personne ne lit. Les deux moitiés de la
   règle anti-silence n'ont donc pas le même remède, et c'est délibéré.
2. **Rendre `effective_repo_root` faillible** — la plus tentante : elle rendrait le type honnête. Casse
   la relecture de tout l'historique (≈ 46/101 Runs de dev), sur des chemins de **lecture**, pour un
   gain purement esthétique.
3. **Un défaut configurable `default_target_repo`** — déplace le problème derrière une indirection de
   plus, et #471 vient de retirer deux réglages d'instance au motif « un axe par écran » : « quel dépôt
   ce Run cible » est un axe **par Run**. Le seul défaut acceptable est l'absence de défaut.
4. **Ne durcir que le frontend** — c'est le statu quo, et c'est le bug. Une règle tenue par un seul
   client n'est pas un invariant : le `curl` d'un agent, un script, un Trigger la contournent.
5. **Supprimer ou rendre optionnel `state.repo_root`** — il garde trois rôles légitimes
   (`<repo_root>/.pdo/pipelines`, bibliothèque, prompts), et le daemon de production sert ses pipelines
   depuis `~/.pdo/app`. Seul son usage comme **cible implicite** disparaît.
6. **Embarquer `source_branch` dans le même durcissement** — même forme, dissymétrie assumée : **le
   HEAD d'un dépôt nommé est un défaut défendable ; le cwd d'un daemon ne l'est pas.**

## Limites acceptées

- **Les Triggers historiques à dépôt nul deviennent dormants** au lieu de tirer. La production en
  compte **zéro** ; le passage `Dangling` fournit le signal rouge et la raison verbatim.
- **`target_repo` reste `Option<String>` pour toujours** dans le payload persisté et dans
  `trigger_store` (colonne `TEXT` nullable). Le log est append-only : la nullabilité est une propriété
  des données historiques, pas une permission accordée à l'écriture. Aucune migration.
- **Le repli de lecture ne pourra jamais être supprimé.** Il n'a pas de date de péremption : tant qu'un
  Run de 2026-07 est lisible, il faut le résoudre.
- Les appelants directs de l'API (scripts, agents, `curl`) doivent nommer `target_repo`. C'est la
  rupture, et elle est intentionnelle.
