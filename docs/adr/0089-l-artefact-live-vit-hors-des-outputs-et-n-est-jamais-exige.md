# L'artefact live vit hors des outputs et n'est jamais exigé

> Statut : accepted (grilling du 2026-10-18, CORE-77). Vocabulaire : CONTEXT.md § *Artefact live*.

## Contexte

Un output pouvait être coché « intermediate » : rempli au fil du travail pour les humains, jamais
vérifié, jamais câblé. Le mélanger aux outputs brouillait le contrat de production d'un Node (un
output est vérifié à la complétion et part sur des edges, sauf celui-là), interdisait à un Node de
publier un suivi qu'on ne lui avait pas déclaré, et ne laissait aucune place à un suivi « attendu
mais facultatif ».

## Décision

1. **L'artefact live est une notion distincte de l'output**, déclarée dans sa propre section du
   Node. Un output reste un contrat vérifié ; un artefact live ne l'est jamais.
2. Un Node peut **déclarer** des artefacts live (nom, type, contenu attendu, schéma de
   frontmatter) : le préambule les annonce pour guider le modèle, mais leur absence ne bloque
   rien et un frontmatter non conforme n'est qu'un avertissement.
3. Un Node peut être **autorisé à publier** des artefacts live non déclarés.
4. Aucune edge ne porte un artefact live et aucun routage ne lit son frontmatter.
5. Un output `intermediate` d'un fichier existant se lit comme un artefact live déclaré.

## Alternatives écartées

- **Garder la case sur l'output** en la renommant « live » : la publication libre et le caractère
  facultatif restent sans place, et chaque règle sur les outputs garde son exception.
