# Libre ERP 0.12.0 — partage SFTP, livraison locale de test

## Réalisé

- Connexion SSH/SFTP native Rust (ssh2/OpenSSL), empreinte SHA256 épinglée avant envoi d’identifiants.
- Configuration et état local protégés par Windows DPAPI, hors archives partagées.
- Archives chiffrées SQLCipher + fichiers, issues du mécanisme de sauvegarde existant.
- Un rédacteur, réservation exclusive par dossier SFTP, jeton de reprise conservé après arrêt.
- Manifestes authentifiés HMAC, versions immuables publiées par renommage sans écrasement, contrôle du parent et de l’historique local.
- Téléchargement, vérification SHA256, validation SQLCipher/audit avant remplacement de la copie active ; ancienne copie conservée dans recovery.
- Points de mutation métier protégés côté Rust sous le verrou Core ; consultation lorsque la réservation n’est pas acquise.
- Paramètres, bandeau, ouverture, fermeture avec envoi/libération ou conservation locale.
- Correction des identifiants sans abandonner le travail, après vérification de l’historique distant ; reprise administrative explicite d’une réservation abandonnée.

## Vérification

40 tests Rust passent ; 4 recettes nécessitant un service externe sont ignorées par défaut. La recette SFTP dédiée a été lancée séparément et passe avec deux stockages SQLCipher indépendants et un serveur SSH/SFTP éphémère sur 127.0.0.1 (données fictives). Elle vérifie aussi la course entre deux connexions pour une seule réservation, les comptes, les pièces jointes et la correction d’un mot de passe avec travail en attente.

12 tests Node passent. Compilation TypeScript/React et recettes d’interface SFTP, fermeture en échec réseau, correction du mot de passe, mise à jour et alignement du tableau de bord passent. Aucun accès à la base de l’application installée n’a été effectué.

EXE et MSI 0.12.0 fabriqués ; signatures updater Ed25519 contrôlées indépendamment et altération rejetée. MSI : ProductName Libre ERP, ProductVersion 0.12.0, UpgradeCode {33607B9F-8084-5DB4-9572-EA078A36B22D}. Le vérificateur de récupération nouvellement compilé valide également le paquet archivé 0.11.1 avec sa preuve signée, sans ouvrir la base.

Livraison : `Livraison-0.12.0-SFTP-test`, avec installateurs, signatures, guide HTML/Markdown, sommes SHA256 et validation.json. Aucune publication GitHub effectuée pour cette version.

## Reproduire sur Windows

- Construction signée : `scripts/Fabriquer-Version-Signee.ps1`, avec le coffre local existant. Aucun renouvellement des clés n’est nécessaire.
- Tests usuels : `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -j 2` et `node --test tests/*.test.mjs`.
- Interface : `node tests/ui-sftp.mjs` (Playwright/Edge).
- Recette réseau fictive : installer Paramiko dans `.validation-deps/sftp` (hors dépendances produit), charger l’environnement de `scripts/Commun.ps1`, puis lancer `python tests/sftp-fixture-server.py` avec ce même Python. Le serveur écoute uniquement sur 127.0.0.1 et utilise un dossier temporaire dédié.

## À valider avant utilisation en équipe

Connexion à l’hébergement choisi et cycle complet sur deux PC physiques : création, connexion du second poste, réservation, modification, envoi, récupération et coupure réseau.

Limites : un rédacteur, archives complètes de 2 Gio maximum, authentification SFTP par mot de passe, version installée uniquement. Sauvegardes manuelles disponibles ; planifications Windows suspendues en mode partagé. Pas de fusion automatique de modifications concurrentes. Les changements de comptes prennent effet après récupération de la version publiée ; authentification et traces de consultation restent locales.
