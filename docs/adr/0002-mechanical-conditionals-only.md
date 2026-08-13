# Conditionals admis, mais mécaniques uniquement

**Superseded by ADR-0011** (conditionnels sur les edges + boucles matérialisées nommées).

Le principe décidé ici tient et vit désormais dans ADR-0011 : le routage conditionnel est admis, mais strictement **mécanique** — prédicats déterministes (`eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `in`, `not_in`) sur le compteur `iter`, des champs de frontmatter d'artefacts ou des variables pipeline ; jamais d'eval LLM, jamais de match sur du contenu libre. ADR-0011 a absorbé le « pourquoi » d'origine (rejet de l'interdiction totale des conditionals comme de l'expression libre) et fixe le placement courant de la condition : clause `when:` sur l'edge, après un détour historique par les ports du nœud `Switch`, depuis supprimé.
