<h1 align="center">PDO</h1>

<p align="center">
  <a href="https://github.com/Loulen/prompt-driven-orchestrator/releases/latest"><img src="https://img.shields.io/github/v/release/Loulen/prompt-driven-orchestrator?style=flat&amp;label=release&amp;color=10b981" alt="Latest release" /></a>
  <a href="../../LICENSE"><img src="https://img.shields.io/badge/license-MIT-10b981?style=flat" alt="License: MIT" /></a>
  <img src="https://img.shields.io/badge/macOS%20%7C%20Linux-10b981?style=flat-square" alt="Plateformes : macOS et Linux" />
</p>

<p align="center">
  <sub><a href="../../README.md">English</a> · <strong>Français</strong></sub>
</p>

<p align="center">
  <strong>L'orchestrateur visuel pour agents de code.</strong><br/>
  Pour les devs 100x qui tiennent encore au travail bien fait.
</p>

<h3 align="center"><a href="#installation"><ins>Installer PDO</ins></a></h3>

<!-- Media slots: `make readme-media` publishes each scene to docs/assets/readme/<scene>.gif with its
     poster docs/assets/readme/<scene>.jpg (scenes: hero, pipelines, routing, outputs, review, triggers,
     stats, orchestration, profiles, skills). Until a scene is published, its slot shows docs/pdo-ui.png.
     In each <picture>: the reduced-motion <source> takes the poster, the other <source> the GIF
     (type="image/gif"), the <img> the poster. README.md and docs/readme/README.fr.md share the media. -->
<p align="center">
  <a href="../../docs/features.md"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/assets/readme/hero.jpg"><source srcset="../../docs/assets/readme/hero.gif" type="image/gif"><img src="../../docs/assets/readme/hero.jpg" alt="Un run de PDO qui tourne : Claude Code travaille dans le terminal du nœud implementer, puis les outputs typés du reviewer" width="960" /></picture></a>
</p>

## Fonctionnalités

<table>
<tr>
<td width="50%" valign="middle">

### Pipelines visuels

Construisez vos workflows d'agents sur un canvas : posez un nœud `implementer`, tirez son arête jusqu'à `end`. Du YAML simple en dessous.

[Docs →](../../docs/features.md#visual-pipelines)

</td>
<td width="50%">
  <!-- scene: pipelines -->
  <a href="../../docs/features.md#visual-pipelines"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/assets/readme/pipelines.jpg"><source srcset="../../docs/assets/readme/pipelines.gif" type="image/gif"><img src="../../docs/assets/readme/pipelines.jpg" alt="Ajout d'un nœud implementer, relié de Start à End sur le canvas" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Routage conditionnel et boucles

Tirez une arête de `reviewer` vers `implementer` et posez `verdict != pass`. Le routage lit des outputs typés, jamais le jugement d'un LLM.

[Docs →](../../docs/features.md#conditional-routing--loops)

</td>
<td width="50%">
  <!-- scene: routing -->
  <a href="../../docs/features.md#conditional-routing--loops"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/assets/readme/routing.jpg"><source srcset="../../docs/assets/readme/routing.gif" type="image/gif"><img src="../../docs/assets/readme/routing.jpg" alt="Une arête de boucle tirée de reviewer vers implementer, avec la condition verdict != pass" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Outputs typés

Chaque nœud déclare ce qu'il transmet (markdown avec frontmatter, listes d'images, fichiers) et PDO le valide avant que le nœud suivant démarre.

[Docs →](../../docs/features.md#typed-outputs)

</td>
<td width="50%">
  <!-- scene: outputs -->
  <a href="../../docs/features.md#typed-outputs"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/pdo-ui.png"><source srcset="../../docs/pdo-ui.png"><img src="../../docs/pdo-ui.png" alt="Les outputs typés du reviewer : un verdict avec un diagramme Mermaid et des captures annotées" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Revue du diff

Commentez n'importe quelle ligne du diff d'un run et envoyez-la à son agent manager. Sa réponse arrive dans le fil, et le correctif repart dans le pipeline.

[Docs →](../../docs/features.md#diff-review)

</td>
<td width="50%">
  <!-- scene: review -->
  <a href="../../docs/features.md#diff-review"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/pdo-ui.png"><source srcset="../../docs/pdo-ui.png"><img src="../../docs/pdo-ui.png" alt="Un commentaire posé sur une ligne du diff, envoyé au manager, puis sa réponse dans le fil" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Triggers

Lancez un pipeline sur un cron, derrière un script de garde : `* * * * *` + `./prod-health-check.sh` transforme une panne de prod en run d'incident, avec le rapport en input.

[Docs →](../../docs/features.md#triggers)

</td>
<td width="50%">
  <!-- scene: triggers -->
  <a href="../../docs/features.md#triggers"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/assets/readme/triggers.jpg"><source srcset="../../docs/assets/readme/triggers.gif" type="image/gif"><img src="../../docs/assets/readme/triggers.jpg" alt="Un trigger cron avec le guard prod-health-check.sh, son test à blanc et son historique de fires" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Stats par modèle

Coût, durée et taux d'échec par modèle et par nœud, tirés de vos propres runs. Voyez ce que vous coûtent vraiment Opus 5.5, Fable 5.1, GPT-5.6 Sol et GLM-5.3 Flash.

[Docs →](../../docs/features.md#run-stats-by-model)

</td>
<td width="50%">
  <!-- scene: stats -->
  <a href="../../docs/features.md#run-stats-by-model"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/assets/readme/stats.jpg"><source srcset="../../docs/assets/readme/stats.gif" type="image/gif"><img src="../../docs/assets/readme/stats.jpg" alt="La page Stats ventilée par modèle et par effort" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Orchestration récursive

Un nœud peut lancer des pipelines enfants depuis sa propre session. Les enfants se rangent sous leur parent dans l'arbre de runs, et l'onglet Orchestration les suit jusqu'au bout.

[Docs →](../../docs/features.md#recursive-orchestration)

</td>
<td width="50%">
  <!-- scene: orchestration -->
  <a href="../../docs/features.md#recursive-orchestration"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/pdo-ui.png"><source srcset="../../docs/pdo-ui.png"><img src="../../docs/pdo-ui.png" alt="Un nœud qui lance des runs enfants, rangés sous leur parent dans l'arbre de runs" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Profils agentiques

Nommez une fois un couple harnais · modèle · effort. Changez le profil, et tous les nœuds qui le suivent basculent, sans nœud à modifier.

[Docs →](../../docs/features.md#agent-profiles)

</td>
<td width="50%">
  <!-- scene: profiles -->
  <a href="../../docs/features.md#agent-profiles"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/assets/readme/profiles.jpg"><source srcset="../../docs/assets/readme/profiles.gif" type="image/gif"><img src="../../docs/assets/readme/profiles.jpg" alt="Un profil agentique modifié, et tous les nœuds qui le suivent qui changent de modèle" width="100%" /></picture></a>
</td>
</tr>
<tr>
<td width="50%" valign="middle">

### Banque de skills

Importez des skills depuis un dépôt ou écrivez-les à la main, puis donnez-les à n'importe quel nœud depuis le sélecteur de skills.

[Docs →](../../docs/features.md#skill-bank)

</td>
<td width="50%">
  <!-- scene: skills -->
  <a href="../../docs/features.md#skill-bank"><picture><source media="(prefers-reduced-motion: reduce)" srcset="../../docs/assets/readme/skills.jpg"><source srcset="../../docs/assets/readme/skills.gif" type="image/gif"><img src="../../docs/assets/readme/skills.jpg" alt="Un skill écrit à la main, des skills importés d'un dépôt local dans la banque, puis l'un d'eux ajouté à un nœud" width="100%" /></picture></a>
</td>
</tr>
</table>

**Aussi dans la boîte :**

- **[Worktrees isolés](../../docs/features.md#isolated-worktrees)** — chaque run a son propre worktree git, et chaque nœud choisit isolé ou partagé.
- **[Sessions live](../../docs/features.md#live-sessions)** — regardez le terminal de n'importe quel nœud, tapez dedans ou reprenez la main.
- **[Sandbox](../../docs/features.md#sandbox)** — faites tourner des nœuds dans un profil conteneur, avec le home du harnais préparé pour vous.
- **[Runs multi-dépôts](../../docs/features.md#multi-repo-runs)** — un seul run qui travaille sur plusieurs dépôts.
- **[Tours guidés](../../docs/features.md#guided-tours)** — l'app vous accompagne dans votre premier pipeline, une étape à la fois.
- **[Page mounts](../../docs/features.md#page-mounts)** — les agents servent prototypes et rapports sous `/pages/<name>/` pour que vous les relisiez.
- **[Service et mise à jour intégrée](../../docs/features.md#service--in-app-update)** — `pdo service install` le garde en marche ; un clic dans la barre d'état le met à jour.
- **Et bien plus** — le [changelog](../../CHANGELOG.md) est la vraie liste des fonctionnalités.

---

## Agents supportés nativement

Fonctionne avec **n'importe quel harnais** : s'il tourne dans un terminal, un descripteur en fait un nœud PDO.

<p>
  <a href="https://docs.anthropic.com/en/docs/claude-code/overview"><kbd><img src="https://www.google.com/s2/favicons?domain=claude.ai&amp;sz=64" alt="Claude Code logo" width="16" valign="middle" /> Claude Code</kbd></a> &nbsp;
  <a href="https://opencode.ai/docs/cli/"><kbd><img src="https://www.google.com/s2/favicons?domain=opencode.ai&amp;sz=64" alt="OpenCode logo" width="16" valign="middle" /> OpenCode</kbd></a> &nbsp;
  <a href="https://docs.github.com/en/copilot/how-tos/set-up/install-copilot-cli"><kbd><img src="https://www.google.com/s2/favicons?domain=github.com&amp;sz=64" alt="GitHub Copilot logo" width="16" valign="middle" /> GitHub Copilot</kbd></a> &nbsp;
  <a href="https://pi.dev"><kbd><img src="https://www.google.com/s2/favicons?domain=pi.dev&amp;sz=64" alt="Pi logo" width="16" valign="middle" /> Pi</kbd></a> &nbsp;
  <a href="../../docs/reference/harnesses.md"><kbd>+ n'importe quel harnais</kbd></a>
</p>

---

## Installation

### Installation — macOS, Linux

```bash
# Homebrew (macOS, Linux)
brew install Loulen/tap/pdo

# ou le script d'installation (Linux, macOS · x86_64, ARM64)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh | sh
```

PDO a besoin de `tmux`, de `git` et d'au moins un harnais d'agent authentifié.

### Démarrer

```bash
pdo daemon            # puis ouvrez http://localhost:5172
pdo service install   # démarre au boot, reste actif après la déconnexion
```

Mises à jour, options du service et toutes les commandes CLI : [docs/reference/cli.md](../../docs/reference/cli.md).

---

## Développer

Envie de contribuer ou de lancer PDO depuis les sources ? Voir [CONTRIBUTING.md](../../CONTRIBUTING.md). La documentation de référence (CLI, reverse proxy, terminal, support des harnais) vit dans [docs/reference/](../../docs/reference/).

## Licence

PDO est libre et open source, sous [licence MIT](../../LICENSE) : utilisez-le, modifiez-le, embarquez-le, hébergez-le, pour tout usage.
