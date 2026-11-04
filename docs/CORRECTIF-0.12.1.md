# Libre ERP 0.12.1 — fond OSM et saisie SFTP

## Problème et correction

La recherche d’adresse Nominatim fonctionnait, mais le mini-plan dépendait d’une extraction dynamique de routes et bâtiments par Overpass. Les essais GET et POST sur le serveur public ont dépassé 14 secondes. L’application utilisait alors systématiquement le secours IGN. Le fonctionnement du site openstreetmap.org ne validait pas cet accès distinct.

Le fond OSM par défaut lit désormais le catalogue de tuiles vectorielles OpenFreeMap, puis au plus quatre tuiles OpenMapTiles. Le décodage MVT est natif en Rust (prost), les routes et bâtiments sont transformés dans le modèle local existant et affichés en SVG. Aucun navigateur distant ni service de capture n’est utilisé. Le rendu reste simplifié et ne reproduit pas exactement le style du site OpenStreetMap.

Le fond obtenu est enregistré dans la fiche, donc sauvegardé et partagé avec elle. Sa consultation ne déclenche aucune requête Internet. Les tuiles utilisées à la demande sont aussi mises en cache localement sous forme chiffrée, pendant sept jours ; le cache est limité à 128 fichiers. Chaque plan utilise au maximum quatre tuiles (3 Mio chacune), sans chargement de régions ou de niveaux de zoom supplémentaires. Le petit catalogue est relu pour retrouver la version courante des tuiles.

Les limites de téléchargement, de décompression, de géométrie et de provenance du serveur sont contrôlées. Les empreintes et les plans précédents ne sont pas effacés si le service échoue. Le secours IGN reste actif. Les anciennes URL publiques connues sont remplacées automatiquement ; un service personnalisé n’est pas modifié. Aucun changement de schéma SQLite n’est nécessaire.

La saisie SFTP accepte les adresses sftp://serveur, sftp://serveur:port et serveur:port. Les identifiants ou chemins mélangés à l’adresse et les ports contradictoires produisent une explication ; ils ne sont pas convertis silencieusement.

## Sources techniques

- OpenFreeMap, fonctionnement et téléchargements hors ligne autorisés : https://openfreemap.org/ et https://openfreemap.org/quick_start/
- Catalogue public : https://tiles.openfreemap.org/planet
- Spécification MVT 2.1 : https://github.com/mapbox/vector-tile-spec/tree/master/2.1
- Attribution : OpenMapTiles et contributeurs OpenStreetMap ; ODbL 1.0. Voir https://openfreemap.org/#attribution

## Recettes

Essai réseau natif sur l’exemple public Hôtel de Ville de Paris : 973 objets, 121 libellés, 0,64 s lors du premier essai. Le rendu a ensuite été vérifié hors réseau avec les géométries réellement reçues. Les données d’aucun bâtiment utilisateur ne sont utilisées pour la recette.

Tests : géométries MVT, emprises polaires et limite de quatre tuiles, rejet des adresses de tuiles externes, cache chiffré, tuiles endommagées ou décompressées trop grandes, saisie SFTP avec préfixe/port et rendu clavier sans réseau. Les contrôles métier existants restent applicables.

Après installation, ouvrir la fiche du bâtiment et cliquer sur **Actualiser le plan (Internet)** pour remplacer volontairement un ancien fond IGN par le nouveau fond OSM. La mise à jour ne remplace pas les plans enregistrés sans action de l’utilisateur.
