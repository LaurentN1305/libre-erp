# Publication GitHub et mises à jour

Dépôt : https://github.com/LaurentN1305/libre-erp — public, licence GPL-3.0-only.

## Première étape
Publication des sources de la version d’essai 0.10.0 et de la liste des corrections connues. La chaîne de contrôle GitHub Actions compile le frontend et exécute les tests Node et Rust sur Windows. Elle dispose uniquement du droit de lecture du dépôt et ne reçoit aucune clé privée.

Les dossiers de livraisons, archives, caches et outils locaux sont exclus. Les fichiers d’exemple des tests ne contiennent pas de données opérationnelles d’une commune. Aucun export PDF utilisateur n’est publié.

## Plan initial de préparation (historique)
1. Corriger le parcours de publication qui impose encore un certificat Authenticode, et adapter la récupération d’une ancienne version pour vérifier une preuve cryptographique sans certificat Windows.
2. Préparer une clé de signature Tauri et une clé Ed25519 du manifeste, conserver les clés privées hors du dépôt et prévoir une sauvegarde éditeur. N’afficher ni clés privées ni mots de passe dans un chat ou un journal.
3. Choisir et configurer le stockage des secrets de publication. Les clés publiques seront intégrées au logiciel ; les clés privées resteront réservées à la fabrication des versions.
4. Adapter le téléchargement aux redirections contrôlées des pièces jointes GitHub Releases. Le code actuel exige un même hôte et refuse les redirections : il ne suffit donc pas de remplacer l’URL du serveur.
5. Préparer deux canaux distincts stable et bêta, avec version, séquence, expiration et signatures vérifiées. Une empreinte SHA-256 non signée ne remplace pas une preuve d’authenticité.
6. Tester le cycle complet sur une installation de test : téléchargement, vérification, sauvegarde avant migration, installation et reprise avec sauvegarde compatible. Ne pas tester sur les données réelles de la commune.
7. Publier une version optimisée et les premiers manifestes signés après la recette.

Ce plan décrit l’état initial de la version 0.10.0. Depuis, les clés de production ont été créées et la version 0.11.0 a été publiée ; consulter l’état de publication en fin de document. Les anciennes versions doivent être remplacées manuellement par cette première version configurée.

## Références techniques
- [Updater Tauri 2](https://v2.tauri.app/plugin/updater/)
- [Signature Windows Tauri](https://v2.tauri.app/distribute/sign/windows/)
- [Permissions minimales des workflows GitHub](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token)


## Préparation de la signature sans certificat Windows

La description About est harmonisée avec le README. Les outils suivants sont prêts :

- `Initialiser-Signatures.ps1` : crée les deux clés dans un nouveau coffre DPAPI du compte Windows, hors projet, sans remplacer un coffre existant. Les fichiers temporaires de génération sont retirés. Les clés publiques sont séparées.
- `Fabriquer-Version-Signee.ps1` : charge les clés uniquement pendant la fabrication, prépare la configuration publique Tauri et utilise le profil release. Aucun certificat Authenticode n’est imposé ; les signatures updater restent obligatoires.
- `Preparer-Version-Signee.ps1 -Canal stable -Sequence N -Sortie DOSSIER_NOUVEAU` : prépare les installeurs, signatures et preuves à partir de sources validées dans Git. Le dossier de sortie est nouveau ; aucune ancienne publication n’est remplacée.
- `Retour-Version.ps1 -Installateur CHEMIN -Preuve PREUVE_JSON` : fait contrôler l’empreinte signée Ed25519 par l’outil de récupération avant de proposer le retour manuel. Une preuve historique expirée reste utilisable pour un retour volontaire ; une recherche de mise à jour refuse un manifeste expiré.

Les manifestes sont distincts par canal et par format : `stable/windows-x86_64-nsis.json`, `stable/windows-x86_64-msi.json`, et les équivalents sous `beta`. Ils sont destinés aux Releases de canaux `stable` et `beta`. Les installeurs versionnés sont destinés à une Release `vVERSION`. Le client autorise les redirections HTTPS des pièces jointes GitHub vers ses hôtes de téléchargement connus, avec un nombre de sauts borné, puis vérifie les signatures et l’empreinte.

La préparation ne téléverse rien sur GitHub. Publier d’abord les paquets et leurs preuves dans la Release de version, puis les deux manifestes dans la Release du canal. Ne jamais publier `signatures.dpapi`, une clé privée, un mot de passe, ou un fichier temporaire du coffre. Chaque publication utilise une séquence strictement croissante pour son canal.

Le coffre DPAPI est lié au compte Windows : une simple copie vers une clé USB ne garantit pas la récupération sur un autre PC. Prévoir une sauvegarde éditeur transportable protégée avant d’activer la première mise à jour publique. Ne pas régénérer une clé déjà intégrée à une application installée.

État de cette préparation : validation sur des clés jetables (coffre, relecture et signature Tauri liée à une version), 34 tests Rust réussis et 11 tests Node réussis. Les deux tests réseau facultatifs restent ignorés. Les clés de production ont été créées après accord explicite de l’éditeur pour leur conservation sur son PC. Leur coffre chiffré a été relu et une signature Tauri de contrôle liée à une version a réussi. Les clés privées ne sont pas publiées. La sauvegarde transportable du coffre et la première publication signée restent à effectuer. L’ancienne version installée ne possède pas encore les clés publiques : une première installation manuelle de la version configurée sera nécessaire.


## Sauvegarder les clés avant la première publication

Sur le PC éditeur, double-cliquer sur `Sauvegarder-Cles-Signature.cmd`. Choisir un nouveau fichier `.erp-cles`, puis saisir deux fois une phrase d’au moins 12 caractères dans la console locale (saisie masquée). Ne jamais communiquer cette phrase dans un chat ou sur GitHub.

Le fichier obtenu est chiffré et relu immédiatement pour vérifier sa cohérence. Le copier sur un support séparé du PC et conserver sa phrase à part. Ce fichier ne doit pas être publié ; il est ignoré par Git.

Sur un autre PC ou compte Windows, `Restaurer-Cles-Signature.cmd` permet de choisir ce fichier et de saisir la phrase. La restauration recrée un coffre Windows à partir des mêmes clés ; elle refuse de remplacer un coffre existant. Les logiciels déjà distribués continuent ainsi de reconnaître les signatures.

Le test automatique du format de sauvegarde vérifie la relecture, le rejet d’une mauvaise phrase et le rejet d’une modification du contenu. Aucune sauvegarde des clés de production n’a été créée automatiquement : sa phrase doit être choisie et conservée par l’éditeur.

## Version 0.11.0 : première installation configurée

La version 0.11.0 intègre les clés publiques du coffre éditeur lors du build de production. Les installeurs sont signés pour les mises à jour Tauri, sans certificat Authenticode Windows. Les données restent dans le même dossier, avec le même identifiant et le même code de mise à niveau MSI.

La recherche s’exécute au démarrage puis toutes les six heures tant que l’application reste ouverte. Le paquet est téléchargé en arrière-plan et ses signatures sont vérifiées avant une notification Windows. Un bandeau dans l’application ouvre les nouveautés et l’installation. L’utilisateur choisit une phrase pour la sauvegarde préalable, puis lance l’installation. La fermeture volontaire de Libre ERP sans accepter la mise à jour ne l’installe pas.

Les nouveautés sont un texte signé, affiché sans interprétation HTML. Pour préparer une publication, modifier le fichier `publication/NOUVEAUTES-0.11.0.txt` ou passer `-Nouveautes CHEMIN` au script de préparation. Les fichiers de canal doivent être renouvelés avant leur expiration (180 jours par défaut), même si la version reste identique ; augmenter alors la séquence. Les preuves de retour manuel restent valables après expiration.

Validation locale de cette évolution : compilation TypeScript/Vite, 12 tests Node, 34 tests Rust réussis (2 tests réseau facultatifs ignorés), et recette d’interface avec notification interne et nouveautés. Une installation réelle suivie d’une mise à jour réelle nécessite encore une recette Windows séparée ; les bases de l’utilisateur ne sont pas ouvertes par ces tests.

## Publication effective du 31 octobre 2026

La [version 0.11.0](https://github.com/LaurentN1305/libre-erp/releases/tag/v0.11.0) est publique : EXE, MSI, signatures Tauri et preuves signées Ed25519. Le tag correspond au commit `1c849938d3333c08eb83223d3b4ed58a70e9e99d`. Le [canal stable](https://github.com/LaurentN1305/libre-erp/releases/tag/stable), séquence 1, est actif ; la page `releases/latest` reste dirigée vers le logiciel 0.11.0. Le canal bêta n’est pas encore alimenté.

Contrôles de livraison : compilation release réussie ; signatures Tauri des deux fichiers vérifiées avec les clés publiques, version liée 0.11.0 et rejet d’un digest modifié ; preuves des deux installeurs acceptées par le vérificateur natif ; version MSI 0.11.0, nom Libre ERP et UpgradeCode conservé ; empreintes SHA-256 calculées par GitHub identiques aux fichiers locaux ; manifestes publics téléchargés puis signatures Ed25519 et nouveautés vérifiées. Les clés privées restent dans le coffre local.

Aucune installation réelle n’a été lancée sur les données utilisateur. Le passage d’une version installée à une version suivante reste à tester sur Windows ; la recette d’interface utilise des données fictives. Une version égale à la version installée ne doit pas afficher de notification de nouvelle version. Conserver une sauvegarde transportable des clés éditeur sur un support séparé.

## Version 0.11.1 publiée pour le test de mise à jour

Version publique : [v0.11.1](https://github.com/LaurentN1305/libre-erp/releases/tag/v0.11.1), sources du tag `b0b8508b113be3f971323850688cc0722aa7a4d2`. Le canal stable utilise désormais la séquence 2. Les deux manifestes publics sont identiques aux fichiers préparés et leurs signatures Ed25519 ont été vérifiées, ainsi que les nouveautés. Les tailles et empreintes SHA-256 calculées par GitHub correspondent aux deux installeurs signés. La page latest pointe vers 0.11.1.

Pour remplacer des manifestes sans suppression, téléverser les nouveaux sous des noms de séquence, archiver les noms standards de l’ancienne séquence dans une première sauvegarde de la Release, puis attribuer les noms standards aux nouveaux fichiers dans une seconde sauvegarde. Une permutation de noms en une seule sauvegarde est refusée par GitHub. Vérifier ensuite les URL standard sans paramètre de cache ; la propagation peut demander quelques instants.

La version installée du poste a été lue sans ouvrir ses données : 0.11.0. La recette réelle doit désormais vérifier notification, nouveautés, sauvegarde, fermeture, installation, réouverture en 0.11.1 et conservation des données. Aucun résultat de cette recette utilisateur n’est encore annoncé.

## Coffre rangé dans les données Windows de Codex

Windows peut rediriger les écritures AppData d’un processus lancé depuis Codex vers `%LOCALAPPDATA%\Packages\OpenAI.Codex_*\LocalCache\Local\LibreERP-Editeur`. Un lanceur ouvert hors de Codex ne retrouve alors pas nécessairement le chemin standard. `Resoudre-Coffre-Signature.ps1` retrouve ce coffre existant lorsque le dossier standard est absent. Il ne déplace pas les fichiers et ne crée aucune clé. Un dossier personnalisé inexistant ne déclenche pas cette recherche, et plusieurs coffres candidats provoquent un arrêt explicite.

Le chargement vérifie que les clés publiques du coffre correspondent à `publication/cles-publiques.json`. La sauvegarde vérifie la présence du coffre avant de demander un emplacement ou une phrase. L’initialisation et la restauration refusent de créer un coffre concurrent lorsqu’un coffre existant a été retrouvé.

Après le correctif, relancer `Sauvegarder-Cles-Signature.cmd` pour choisir un nouveau fichier `.erp-cles` et sa phrase locale. Il s’agit de sauvegarder les clés déjà utilisées, pas d’en générer de nouvelles. Ne jamais saisir cette phrase dans un chat ni publier l’archive.

Validation : recette de localisation sur des fichiers factices (absence, coffre MSIX, dossier personnalisé, priorité du dossier standard), et chargement réel du coffre existant sous Windows PowerShell 5.1 avec correspondance des clés publiques. Aucune clé privée n’est affichée ou publiée. La sauvegarde de production doit encore être effectuée par l’éditeur avec sa phrase choisie localement.
