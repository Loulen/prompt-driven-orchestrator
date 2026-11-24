# La Public URL a un défaut, l'adresse de la machine

> Statut : accepted. Vocabulaire : CONTEXT.md § *Configuration d'instance* (URL publique).
> Amende ADR-0015 (configuration d'instance et précédence `stored → env → default`).

## Contexte

La Public URL est l'adresse à laquelle les humains ouvrent l'instance. Les agents la reçoivent
(`PDO_PUBLIC_URL` et une section du préambule), `pdo page mount` imprime ses liens sous elle, et
Settings en tire l'URL de `/metrics`. Jusqu'ici, elle n'avait pas de défaut : `stored → env →
none`. Sans réglage, les agents et `pdo page mount` imprimaient des liens `localhost`, que seul
l'hôte du daemon sait ouvrir, alors que le daemon écoute sur toutes les interfaces par défaut et
que Settings affichait déjà l'URL de `/metrics` avec l'adresse de la machine. Le champ paraissait
obligatoire pour un résultat que l'instance peut proposer seule.

## Décision

1. **La Public URL a toujours une valeur** : `stored → env PDO_PUBLIC_URL → default`, comme les
   autres réglages d'instance (ADR-0015). Le résultat de la résolution n'est plus optionnel.
2. **Le défaut est l'adresse de la machine**, calculé une fois au démarrage à partir de l'adresse
   d'écoute du daemon :
   - adresse non spécifiée (`0.0.0.0`, `::`, le défaut de `pdo daemon`) : `http://<nom de la
     machine>:<port>`, avec `localhost` si le système ne donne pas de nom ;
   - adresse de loopback : `http://localhost:<port>`, puisque seul l'hôte joint le daemon ;
   - adresse précise : cette adresse (`http://10.0.0.4:5272`, `http://[fd00::4]:5272`).

   Le nom de machine est celui de l'identité de l'instance dans la télémétrie
   (`service.instance.id`).
3. **Les consommateurs lisent la valeur effective** : la section « Public URL » du préambule est
   toujours présente, `PDO_PUBLIC_URL` est toujours posée pour les sessions de Node et du Pipeline
   Manager, `pdo page mount` imprime ses liens sous elle, et l'interface en tire le lien copié d'un
   Page mount et l'URL de `/metrics`, au lieu de l'adresse du navigateur.
4. **`GET /settings` divulgue le défaut** (`public_url.default`) avec la valeur effective et la
   source (`stored`, `env` ou `default`). Settings › General montre dans le champ vide l'adresse
   qu'il utilisera, en placeholder, et la source dans l'infobulle du nom. L'interface ne qualifie
   pas le défaut (pas de « deduced ») : c'est la valeur de l'instance tant que personne n'en règle
   une autre.

## Alternatives écartées

- **Retenir l'origine du premier navigateur** qui ouvre l'instance : instable, elle dépend d'une
  visite, et plusieurs postes l'ouvrent sous plusieurs adresses.
- **Garder le champ obligatoire** : un lien `localhost` ne sert à personne d'autre que l'hôte, et
  l'instance connaît une meilleure adresse.

## Conséquences

- Les liens des agents et de `pdo page mount` s'ouvrent depuis un autre poste sans réglage.
- Derrière un reverse proxy, en HTTPS ou sur un VPN, l'opérateur règle toujours la Public URL :
  PDO ne détecte ni proxy, ni HTTPS, ni certificat.
- Une installation qui pose `PDO_PUBLIC_URL` ne change pas : l'environnement garde sa place entre
  la valeur réglée et le défaut.
