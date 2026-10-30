# Libre ERP 0.10.0 — M8 Exports et import CSV

## Installation
1. Dans votre version actuelle, faites une sauvegarde depuis les paramètres.
2. Fermez Libre ERP.
3. Lancez LibreERP-M8-essai-setup.exe et suivez l’installation.

Tous les modules précédents sont inclus. Le MSI est une autre méthode d’installation : ne lancez pas les deux installateurs. Votre installation actuelle n’a pas été remplacée automatiquement lors de la préparation de cette livraison.

Il s’agit toujours d’une version d’essai sans certificat Windows, conformément au choix de ne pas acheter de signature Authenticode. Les mises à jour publiques et leur signature cryptographique seront configurées séparément, avec le dépôt GitHub à choisir.

Les données installées restent dans %LOCALAPPDATA%\fr.securiteerp.desktop. La base est chiffrée et passe au schéma 13. Une ancienne version ne doit pas ouvrir directement cette base migrée : conservez la sauvegarde faite avant la mise à jour pour un retour éventuel.

## Bouton Alertes et présentation
- L’icône et le titre Libre ERP sont centrés dans la colonne, au-dessus du logo de la commune.
- Le bouton Alertes indique le nombre total de rappels ouverts accessibles au compte, y compris ceux déjà pris en charge.
- Couleur selon le niveau le plus élevé : bleu pour information, jaune pour surveillance, orange pour important, rouge pour critique. Vert sans compteur si aucun rappel n’est ouvert.
- Le libellé accessible et l’infobulle indiquent aussi l’état. Le compteur se rafraîchit toutes les minutes lorsque la session est ouverte.

## Exports
Ouvrez « Exports / import » depuis le tableau de bord, ou Paramètres → Exports / import. Les rôles exploitant, responsable sécurité et responsable technique disposent de l’export ; les droits par bâtiment et les restrictions budgétaires restent appliqués.

Choisissez les données, éventuellement un bâtiment, puis affichez l’aperçu. Vous pouvez enregistrer un fichier Excel .xlsx, CSV ou PDF.

Les listes couvrent les bâtiments, les entrées du registre, les interventions, l’inventaire, les consommations/relevés, les contrats, les documents et les alertes. L’aperçu affiche les 100 premières lignes ; l’export conserve toutes les lignes sélectionnées, dans la limite annoncée (20 000 lignes, 2 000 pour le PDF). Les exports ne contiennent pas les pièces jointes elles-mêmes. Les exports de documents et d’alertes ne reprennent pas les plafonds d’affichage des vues ; si la limite générale de l’export est dépassée, un filtrage est demandé.

Le PDF complet du registre A à I avec ses annexes reste dans le module Registre. Le nouveau rapport de liste constitue un complément, sans remplacer ce registre.

Pour imprimer, enregistrez le PDF, ouvrez-le avec votre lecteur habituel puis utilisez Ctrl+P. Les PDF sont créés localement avec le logo de la commune lorsqu’il est renseigné. Le logo est relu au moment de la création.

Excel conserve les montants comme nombres et les numéros de série comme texte, afin de préserver leurs zéros initiaux. Les cellules CSV pouvant être prises pour des formules reçoivent une apostrophe de protection. Une cellule vide peut être non renseignée ou masquée par vos droits : elle ne vaut pas montant nul.

Les exports sont des fichiers lisibles hors de Libre ERP. Ils ne restent pas chiffrés par l’application. Rangez et transmettez-les avec les précautions habituelles pour vos documents municipaux.

Les consommations exportées sont les valeurs brutes : un index cumulatif ne doit pas être additionné comme une consommation de période. Le type, l’unité, les remises à zéro et les annulations sont indiqués.

## Import CSV d’un inventaire
1. Ouvrez « Importer un inventaire CSV » et choisissez un bâtiment non archivé dans lequel vous pouvez modifier l’inventaire.
2. Enregistrez le modèle CSV. Consultez les codes de catégories et les localisations dans la rubrique dépliable.
3. Remplissez le fichier dans votre tableur et enregistrez-le en CSV UTF-8.
4. Choisissez le fichier pour afficher l’aperçu. Corrigez toutes les erreurs avant de le relire.
5. Vérifiez les lignes, cochez la confirmation puis ajoutez les équipements.

Colonnes obligatoires : designation et categorie. Les en-têtes doivent correspondre au modèle. Les colonnes facultatives peuvent être supprimées ou laissées vides. Les références Libre ERP seront attribuées automatiquement.

Dates : AAAA-MM-JJ. Coût : euros, avec deux décimales maximum, virgule ou point. État : neuf, bon, usé, défectueux ou hors service. Entretien : une fréquence en jours ou en mois, pas les deux. Une localisation doit déjà exister dans l’inventaire ; utilisez son identifiant si plusieurs emplacements portent le même nom.

Sont ignorés et signalés : une ligne identique déjà importée dans ce bâtiment, une répétition identique dans le CSV, ou un numéro de série avec le même fabricant déjà présent dans le bâtiment ou dans le lot. Aucun équipement existant n’est écrasé. Pour plusieurs objets identiques sans numéro de série, créez-les séparément ou différenciez-les avant l’import.

Limites : 2 Mio, 5 000 lignes de données, 20 000 caractères par cellule. Séparateurs acceptés : point-virgule, virgule, tabulation. Les champs entre guillemets peuvent contenir des retours à la ligne. Les numéros de ligne de l’aperçu désignent les enregistrements logiques du CSV, après l’en-tête.

La validation et les droits sont vérifiés à nouveau lors de la confirmation. L’ajout du lot et son historique sont écrits dans une transaction SQLite unique. Une erreur pendant cette transaction annule tous ses ajouts. Les comptes rendus et traces d’import sont enregistrés dans l’audit. Vous pouvez ensuite compléter les fiches, ajouter leurs photos et leurs pièces jointes dans l’inventaire.

Cet import sert à reprendre un inventaire tabulaire. Pour transférer un bâtiment complet entre utilisateurs, utilisez toujours Paramètres → Partager, avec le fichier chiffré .erp-partage.

## Version portable et récupération
Décompressez entièrement LibreERP-0.10.0-portable.zip puis lancez Portable\LibreERP-portable.exe. Les données portables sont dans le dossier Donnees à côté du lanceur. Fermez l’application avant de copier tout ce dossier. Les tâches Windows planifiées restent réservées à la version installée.

LibreERP-M8-recuperation.exe et Portable\LibreERP-recuperation.exe servent à récupérer une sauvegarde si le lancement normal échoue ; ce ne sont pas les lanceurs habituels.

## Vérifications à faire sur votre poste
- Vérifier le centrage avec votre logo de commune.
- Vérifier le compteur et la couleur du bouton Alertes.
- Exporter les interventions d’un bâtiment vers Excel puis ouvrir le fichier.
- Enregistrer et imprimer un PDF.
- Importer un petit inventaire test, puis relire le même CSV : il ne doit pas recréer les mêmes équipements.
- Vérifier le comportement avec un compte limité à un bâtiment.

## Recompiler sous Windows
Les sources complètes figurent dans LibreERP-M8-sources.zip. Les dépendances doivent être préparées avec les scripts du projet (Rust/MSVC, Node, dépendances frontend, OpenSSL et WebView2). Depuis le dossier du projet :

```powershell
. ./scripts/Commun.ps1
node scripts/frontend.mjs build
cargo test --locked --offline --manifest-path src-tauri/Cargo.toml -j 2
cargo build --locked --offline --manifest-path src-tauri/Cargo.toml --bins --features custom-protocol -j 2
Copy-Item src-tauri/target/debug/securite-erp-recovery.exe src-tauri/resources/securite-erp-recovery.exe -Force
node node_modules/@tauri-apps/cli/tauri.js bundle --debug --features custom-protocol --bundles nsis,msi --config src-tauri/tauri.distribution.json --ci
```

Les commandes hors ligne supposent les caches de dépendances déjà disponibles. NSIS produit le .exe et WiX le .msi. Les sources n’incluent ni outils téléchargés, ni données de commune, ni clés privées. MSIX est un autre format que MSI.
