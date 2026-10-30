# Publication GitHub et mises à jour

Dépôt : https://github.com/LaurentN1305/libre-erp — public, licence GPL-3.0-only.

## Première étape
Publication des sources de la version d’essai 0.10.0 et de la liste des corrections connues. La chaîne de contrôle GitHub Actions compile le frontend et exécute les tests Node et Rust sur Windows. Elle dispose uniquement du droit de lecture du dépôt et ne reçoit aucune clé privée.

Les dossiers de livraisons, archives, caches et outils locaux sont exclus. Les fichiers d’exemple des tests ne contiennent pas de données opérationnelles d’une commune. Aucun export PDF utilisateur n’est publié.

## Étapes avant la première mise à jour publique
1. Corriger le parcours de publication qui impose encore un certificat Authenticode, et adapter la récupération d’une ancienne version pour vérifier une preuve cryptographique sans certificat Windows.
2. Préparer une clé de signature Tauri et une clé Ed25519 du manifeste, conserver les clés privées hors du dépôt et prévoir une sauvegarde éditeur. N’afficher ni clés privées ni mots de passe dans un chat ou un journal.
3. Choisir et configurer le stockage des secrets de publication. Les clés publiques seront intégrées au logiciel ; les clés privées resteront réservées à la fabrication des versions.
4. Adapter le téléchargement aux redirections contrôlées des pièces jointes GitHub Releases. Le code actuel exige un même hôte et refuse les redirections : il ne suffit donc pas de remplacer l’URL du serveur.
5. Préparer deux canaux distincts stable et bêta, avec version, séquence, expiration et signatures vérifiées. Une empreinte SHA-256 non signée ne remplace pas une preuve d’authenticité.
6. Tester le cycle complet sur une installation de test : téléchargement, vérification, sauvegarde avant migration, installation et reprise avec sauvegarde compatible. Ne pas tester sur les données réelles de la commune.
7. Publier une version optimisée et les premiers manifestes signés après la recette.

Le dépôt public et les contrôles automatiques ne signifient pas que les mises à jour sont actives dans les versions installées. Les livraisons actuelles restent des essais non signés Authenticode ; aucune clé de production n’a été créée ou déposée pendant cette préparation.

## Références techniques
- [Updater Tauri 2](https://v2.tauri.app/plugin/updater/)
- [Signature Windows Tauri](https://v2.tauri.app/distribute/sign/windows/)
- [Permissions minimales des workflows GitHub](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token)
