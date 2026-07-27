# PDO sandbox base image.
#
# Ce Dockerfile est content-hashé par le daemon pour dériver le tag
# `pdo-sandbox:h-<hash>` (issue #405) : ses octets doivent rester stables/reproductibles.
# La base est pinnée à un TAG (pas :latest) pour cette raison. Un tag flotte quand même
# (Canonical republie ubuntu:24.04 avec des patches), donc un hash de Dockerfile stable
# ne garantit PAS des layers bit-à-bit identiques ; pinner par digest (ubuntu:24.04@sha256:...)
# si la repro bit-à-bit devient nécessaire. Le tag est le défaut pragmatique : lisible,
# ramasse les patches sécu, et le daemon n'a besoin que du *texte du Dockerfile* stable.
FROM ubuntu:24.04

# Un seul layer apt, listes nettoyées, pour garder l'image petite.
#   git, ca-certificates, curl -> outillage requis (curl lance aussi l'installeur)
#   ripgrep                    -> backend de recherche de Claude Code (cf. USE_BUILTIN_RIPGREP)
#   sudo                       -> l'utilisateur runtime peut `apt install` (NOPASSWD ci-dessous)
# NON installés ici (fournis par l'HÔTE au runtime, par design #403) :
#   tmux -> tourne sur l'HÔTE, jamais dans le conteneur
#   pdo  -> le binaire `pdo` de l'hôte est bind-monté read-only au runtime
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      git \
      ca-certificates \
      curl \
      ripgrep \
      sudo \
 && rm -rf /var/lib/apt/lists/*

# sudo sans mot de passe pour tout utilisateur. Le conteneur tourne en uid:gid hôte
# (`--user`), un uid arbitraire, donc la règle n'est pas liée à un nom d'utilisateur.
# NOTE (validation réelle) : sudo appelle getpwuid() et abandonne avec "you do not exist
# in the passwd database" si l'uid n'a pas d'entrée /etc/passwd. ubuntu:24.04 livre un
# user `ubuntu` en uid 1000, donc le cas laptop courant (uid 1000) résout. Pour tout autre
# uid hôte, le RUNTIME (#406) doit injecter une entrée passwd — hors périmètre de l'image.
RUN echo 'ALL ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/pdo-nopasswd \
 && chmod 0440 /etc/sudoers.d/pdo-nopasswd \
 && visudo -cf /etc/sudoers.d/pdo-nopasswd

# Installe Claude Code via l'installeur natif officiel (recommandé par Anthropic ; npm n'est
# qu'un chemin de compat). L'installeur pose un launcher à $HOME/.local/bin/claude (symlink
# vers un binaire self-contained), sous le home de root pendant le build -> PAS sur le PATH
# d'un uid runtime arbitraire. On résout le vrai binaire et on le copie dans /usr/local/bin,
# lisible+exécutable par tout uid, puis on supprime l'arbre d'install de root.
RUN curl -fsSL https://claude.ai/install.sh | bash \
 && install -D -m 0755 "$(readlink -f /root/.local/bin/claude)" /usr/local/bin/claude \
 && rm -rf /root/.local /root/.claude /root/.cache

# Désactive l'auto-update de Claude Code via ENV, volontairement (pas un settings.json baké) :
# le runtime bind-monte HOME/.claude depuis le staged home (#404), ce qui SHADOWERAIT un
# settings.json baké. Une env var posée dans l'image survit au mount et s'applique à l'uid runtime.
#   DISABLE_AUTOUPDATER=1 -> stoppe le check de mise à jour en arrière-plan.
#   USE_BUILTIN_RIPGREP=0 -> utilise le `rg` système apt-installé.
ENV DISABLE_AUTOUPDATER=1 \
    USE_BUILTIN_RIPGREP=0

CMD ["claude"]
