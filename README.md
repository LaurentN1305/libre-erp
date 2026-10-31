# Libre ERP

Libre ERP est une application Windows hors ligne dédiée au suivi des bâtiments ERP (établissements recevant du public). Elle centralise la maintenance, les contrats d’entretien, les interventions, les équipements et le registre de sécurité, ainsi que les documents, les échéances et l’historique de chaque bâtiment.

**État : version 0.11.1, schéma SQLite 13.** Le logiciel fonctionne hors ligne pour ses données métier. La recherche cartographique et la vérification de mises à jour sont des actions réseau facultatives.

## Télécharger Libre ERP pour Windows

**[Télécharger la dernière version Windows](https://github.com/LaurentN1305/libre-erp/releases/latest)**

Choisir le fichier `LibreERP_VERSION_x64-setup.exe`, le lancer et suivre l’installation. Le MSI est disponible pour les déploiements administrés. Les archives « Source code » contiennent les sources, pas le logiciel à installer.

La première version avec mises à jour signées est **0.11.0**. Une installation manuelle est nécessaire pour les utilisateurs des versions précédentes. Les versions suivantes pourront être proposées dans l’application, avec leurs nouveautés et une sauvegarde préalable.

## Fonctions disponibles
- Tableau de bord et fiches multi-bâtiments, plusieurs types ERP par bâtiment.
- Registre de sécurité en neuf rubriques, historique et export PDF.
- Interventions, priorités, suivi, budgets et pièces jointes.
- Inventaire, entretien et relevés de consommation ; QR facultatifs.
- Contrats, prestataires et documents avec versions et échéances.
- Alertes, prise en charge, escalade et notifications Windows.
- Exports Excel/CSV/PDF et import CSV d’inventaire avec aperçu.
- Comptes locaux, rôles et périmètres, TOTP facultatif, audit, sauvegarde et échange chiffré de bâtiments.
- Version installée ou portable ; logo de la commune personnalisable.

Les données réelles, les comptes, les sauvegardes, les exports et les clés de signature ne font pas partie du dépôt. Les exemples des tests sont fictifs.

## Installation et documentation
Le [guide de la version 0.10.0](docs/M8-LibreERP.md) décrit les fonctions et les essais. Les installateurs ne sont pas encore publiés dans les Releases de ce dépôt.

Les [corrections connues](docs/SUIVI-CORRECTIONS.md) concernent l’alignement des actions du tableau de bord, les PDF récapitulatifs, la présentation du registre et la fiabilité d’OpenStreetMap. Le recours IGN est disponible.

Ce projet facilite la tenue des informations ; son utilisation et un export ne constituent pas une certification de conformité d’un établissement.

## Développement sous Windows x64
Installer Node.js 24, pnpm 11.25.0, Rust MSVC (1.90 minimum), les outils C++ de Visual Studio avec le SDK Windows, et Perl pour la compilation OpenSSL/SQLCipher. WebView2 est nécessaire à l’exécution.

```powershell
pnpm install --frozen-lockfile
pnpm run build
pnpm test
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -j 2
cargo build --locked --manifest-path src-tauri/Cargo.toml --bins --features custom-protocol -j 2
Copy-Item src-tauri/target/debug/securite-erp-recovery.exe src-tauri/resources/securite-erp-recovery.exe -Force
pnpm exec tauri bundle --debug --features custom-protocol --bundles nsis,msi --config src-tauri/tauri.distribution.json --ci
```

La première compilation télécharge des dépendances. L’application utilise ensuite sa base SQLCipher et ses fichiers locaux chiffrés. Les installateurs NSIS (.exe) et WiX (.msi) sont produits dans `src-tauri/target/debug/bundle`. Le correctif local de `openssl-sys` sous `src-tauri/vendor` doit être conservé.

Les recettes d’interface sous `tests/ui-*.mjs` utilisent actuellement un environnement Playwright local ; elles ne sont pas encore exécutées sur GitHub Actions. Les tests métier utilisent des bases temporaires.

## Données et mises à jour
La version installée conserve ses données dans `%LOCALAPPDATA%\fr.securiteerp.desktop`. Le mode portable conserve son dossier `Donnees` à côté du lanceur. Utiliser les fonctions de sauvegarde, restauration et partage, plutôt que copier uniquement le fichier de base.

La signature payante Authenticode Windows n’est pas requise pour ce projet. Les mises à jour automatiques nécessitent leurs propres signatures cryptographiques : le canal stable est actif depuis la version 0.11.0. Voir le [plan de publication](docs/PUBLICATION.md).

## Licence
Copyright 2026 LaurentN1305 et les contributeurs. Le code propre au projet est distribué sous **GNU GPL version 3 uniquement**, voir [LICENSE](LICENSE). Les dépendances et éléments tiers conservent leurs licences et notices respectives. Les fichiers dérivés de `openssl-sys` conservent notamment leurs mentions originales.
