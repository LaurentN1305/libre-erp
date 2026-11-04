# Correctif 0.12.2 : provenance des plans vectoriels

La version 0.12.1 autorisait `osm-vector` dans le modèle Rust et le frontend mais les deux déclencheurs SQLite de la migration 0008 n’autorisaient que `osm` et `ign`. La récupération réussissait, puis l’écriture échouait avec SQLite 1811 / `Unknown map provenance`.

La migration 0014 remplace ces deux déclencheurs pour autoriser les trois provenances. Les migrations publiées ne sont pas modifiées, ni les données existantes. Le numéro de schéma passe de 13 à 14. Le mécanisme existant sauvegarde la base chiffrée avant migration ; les contrôles d’intégrité restent actifs. Les provenances inconnues restent rejetées à l’insertion et à la mise à jour.

Le test de régression utilise une vraie base SQLCipher au schéma 13, reproduit l’erreur, applique les migrations, vérifie la conservation des anciens plans OSM/IGN et l’enregistrement vectoriel sur fiche nouvelle et existante, puis ferme et rouvre la base. Il vérifie également les refus de provenances inconnues et le contrôle d’intégrité.

Tous les postes partageant un espace SFTP doivent être mis à jour avant de reprendre les échanges. Les anciennes versions refusent les archives de schéma supérieur, sans altérer les données.
