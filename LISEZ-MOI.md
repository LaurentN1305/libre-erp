# Libre ERP

Application Windows de gestion des bâtiments, du registre de sécurité et de la maintenance des établissements recevant du public. Développée avec Tauri 2, Rust, React et TypeScript.

**État : version d’essai 0.10.0, schéma SQLite 13.** Le logiciel fonctionne hors ligne pour ses données métier. La recherche cartographique et la vérification de mises à jour sont des actions réseau facultatives.

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


## Les trois fichiers à connaître

- **Lancer-Application.cmd** : ouvre l'application depuis ses sources. Le premier lancement peut prendre plusieurs minutes.
- **Essayer-Application.cmd** : ouvre directement le binaire de test déjà compilé, s'il est présent.
- **Fabriquer-Installateurs.cmd** : vérifie le projet et fabrique les installateurs Windows `.exe` et `.msi`.
- **Verifier-Installation.cmd** : indique si les outils nécessaires à la compilation sont disponibles.

Double-cliquez sur le fichier souhaité. Les commandes sont préparées pour vous. Les installateurs obtenus se trouvent dans `src-tauri/target/release/bundle`, sous-dossiers `nsis` et `msi`.

La compilation initiale nécessite Internet pour récupérer les dépendances et les composants du packaging. **L'application installée fonctionne ensuite hors ligne.** Les outils de développement ne sont pas nécessaires chez ses utilisateurs.

## Premier démarrage

1. Indiquez le nom de votre collectivité ou organisation.
2. Créez votre compte exploitant, avec un identifiant simple et un mot de passe de 12 caractères minimum.
3. Dans **Sauvegardes**, créez une archive de test et notez sa phrase de récupération dans un endroit sûr.
4. Dans **Mon compte**, vous pouvez activer une application d'authentification TOTP et conserver les codes de récupération.
5. Dans **Utilisateurs**, ajoutez les membres de votre équipe et choisissez leurs rôles.

Il n'existe aucun compte ou mot de passe par défaut. Une inactivité de 30 minutes impose une nouvelle connexion. Cinq tentatives de connexion incorrectes verrouillent temporairement le compte pendant 15 minutes.

## Vos données

Les données sont placées dans `%LOCALAPPDATA%\fr.securiteerp.desktop`. La base et les pièces jointes sont chiffrées. La clé de fonctionnement est protégée par Windows pour le compte utilisateur actuel.

**Une copie du seul fichier de base ne constitue pas une sauvegarde transportable.** Utilisez le bouton de sauvegarde : l'archive contient ce qui est nécessaire pour restaurer sur un autre poste, avec sa phrase de récupération.

Le dossier du projet peut être sur KDrive ; la base de l'application est dans le dossier AppData local du poste, pas dans ce projet. Ne déplacez pas la base active vers un dossier synchronisé.

## Sauvegarder et restaurer

Les archives ZIP contiennent des données chiffrées, y compris leurs métadonnées sensibles. Le nom de l'archive révèle uniquement le produit et la date. Une phrase d'au moins 12 caractères les protège.

Choisissez de préférence un dossier de sauvegarde sur un autre support. Une sauvegarde sur le même disque ne protège pas contre sa perte.

L'activation de la sauvegarde quotidienne crée une tâche Windows pour votre compte. Elle permet de sauvegarder application fermée tant que votre session Windows reste ouverte. L'application assure aussi le rattrapage après l'heure prévue. Si Windows refuse de créer la tâche, un message l'indique. Les archives ne sont pas supprimées automatiquement dans cette version.

Pour restaurer depuis l'application : indiquez la phrase de l'archive, sélectionnez le ZIP puis confirmez. Une sauvegarde des données actuelles est exigée avant remplacement. L'application redémarre et vous vous reconnectez avec un compte présent dans l'archive.

Si l'application ne démarre plus, **Recuperer-Donnees.cmd** ouvre l'outil indépendant livré avec les installateurs. Il vérifie l'archive et conserve les fichiers précédents dans le dossier de récupération avant remplacement. Dans une installation distribuée, l'outil se trouve dans le sous-dossier `recovery` des ressources de l'application.

## Installateurs de test et publication

Le script standard produit une **version de test non signée**. Windows peut afficher un avertissement sur son éditeur. Ne la diffusez pas comme une version de production.

La signature Windows payante ne sera pas utilisée. Les mises à jour exigeront des clés de signature cryptographique et une publication HTTPS ; cette configuration reste à réaliser. Aucun achat n'a été effectué et aucun certificat n'est inventé. La procédure technique est dans `docs/P0-technique.md`. Tant que ce paramétrage n'est pas fait, l'écran Mises à jour l'indique et ne prétend pas avoir vérifié une signature.

## Outils de compilation, sur un autre ordinateur

Les sources nécessitent Windows x64, [Node.js LTS](https://nodejs.org/), [Rust MSVC](https://rustup.rs/), [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) avec C++ et SDK Windows, et [Strawberry Perl](https://strawberryperl.com/) pour OpenSSL/SQLCipher. Les scripts utilisent pnpm 11.25.0 et le fichier de verrouillage fourni. NASM est facultatif ici : OpenSSL peut compiler sans assembleur.

Une chaîne Rust et Perl portable a été préparée dans `.tools` pour ce poste ; elle n'est pas incluse dans les installateurs et n'est pas ajoutée au PATH général de Windows.

## Limites de cette étape

P0 prépare les fondations ; il ne constitue pas encore une application de gestion ERP complète. Les périmètres de rôles par bâtiment sont prévus dans le schéma mais leur interface arrivera avec les bâtiments. La recherche Ctrl+K porte pour l'instant sur les paramètres. La conformité réglementaire et l'accessibilité complète seront évaluées sur les modules finaux, sans certification anticipée.

Les validations réellement exécutées sont consignées dans `docs/VALIDATION-P0.md`.
