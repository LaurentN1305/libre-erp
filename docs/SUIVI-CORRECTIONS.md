# Corrections à reprendre après M8

Retours sur la version 0.10.0, enregistrés le 30 octobre 2026. Ces points sont constatés et restent ouverts ; les modules fonctionnels sont validés pour poursuivre la préparation de la publication.

## 1. Actions du tableau de bord
Les boutons Documents, Contrats et prestataires, Actualiser, Ajouter un bâtiment, Partager et Exports/import ne sont pas alignés. Regrouper les actions dans une barre cohérente, avec retour à la ligne propre aux petites largeurs. Supprimer les flottants indépendants responsables du décalage.

Critère : alignement stable aux largeurs 1024, 1280 et 1440 px, ordre clavier identique à l’ordre visuel, aucune action superposée ou isolée au-dessus des autres.

## 2. PDF récapitulatif des interventions
L’export actuel empile les champs en fiches longues. Préférer un tableau récapitulatif lisible : référence, bâtiment, intitulé, priorité, état, échéance, intervenant et, selon les droits, budget. Répéter les en-têtes, maîtriser les largeurs et les sauts de page. Réserver les longs commentaires à une annexe ou une fiche détaillée.

Critère : lecture rapide sur un exemple de plusieurs interventions, titres longs et budgets, avec contrôle visuel du PDF imprimé. Ne pas publier les exports réels d’une commune dans le dépôt.

## 3. Présentation du registre de sécurité
Revoir le PDF A à I pour évoquer un vrai registre : couverture rouge, identité de l’établissement et de la commune, sommaire, intercalaires, tableaux adaptés à chaque rubrique, pagination, espaces de signature lorsque pertinents et annexes identifiées.

Étape préalable : rechercher des modèles publics en ligne et citer les sources d’inspiration, sans reproduire de modèle commercial. Conserver tous les champs, mentions, horodatages et justificatifs existants. Faire valider une maquette avant de remplacer la présentation.

## 4. OpenStreetMap
Le fonctionnement reste défaillant chez l’utilisateur malgré les corrections précédentes. IGN constitue actuellement le recours fonctionnel. Reprendre le diagnostic des services, délais et messages d’erreur, avec un exemple public reproductible puis une recette sur le poste utilisateur. Conserver les plans déjà enregistrés et distinguer clairement la provenance OSM ou IGN.

Critère : géocodage et récupération du fond testés séparément ; échec explicite sans bloquer l’enregistrement de la fiche ; fonctionnement hors ligne d’un plan déjà conservé ; confirmation du résultat sur le poste concerné.

## Reprise dans la version locale 0.11.1 (31 octobre 2026)

1. Barre d’actions corrigée : six boutons regroupés, sans flottants. Recette d’interface réussie à 1024, 1280 et 1440 px, ordre clavier et visuel identique, aucun débordement.
2. PDF des interventions : récapitulatif A4 paysage, en-têtes répétés, intervenants ajoutés, type / avancement et dates regroupés, budgets selon les droits. Descriptions intégrales dans une table complémentaire. Exemple fictif : 14 interventions sur 5 pages.
3. PDF du registre : couverture rouge et identité, logo communal, sommaire A-I paginé, tableaux récapitulatifs et fiches, versions et archives, visa manuscrit facultatif et inventaire d’annexes. Exemple fictif : 13 pages, toutes rendues et contrôlées. Tests supplémentaires sur textes longs et annexes. La validation esthétique par l’utilisateur reste attendue.
4. Cartographie partiellement corrigée : remplacement des anciennes adresses publiques OSM par le service de lecture Overpass, requête GET bornée et suppression du contournement silencieux d’OSM pendant cinq minutes. Un diagnostic volontaire teste séparément adresse OSM, fond OSM et IGN sur un exemple public. La recette native confirme Nominatim + IGN, mais Overpass expire après 14 s depuis ce poste. L’accès au serveur OSM n’est donc pas annoncé comme rétabli. Aucun fond enregistré n’est supprimé.

### Sources de conception

- [Registre public de la direction de la Sécurité civile de Nouvelle-Calédonie](https://securite-civile.gouv.nc/sites/default/files/documents/Doc_Registre%20de%20securite%20incendie%20-%20ERP%20du%201er%20groupe.pdf) : inspiration de structure uniquement (identité, sommaire, tableaux de suivi). Son contenu juridique n’est pas repris. Le modèle Libre ERP conserve son propre schéma A-I.
- [Fiche du ministère de l’Éducation nationale](https://cache.media.education.gouv.fr/file/ONS/02/0/fpo_registre_securite_incendie_548020.pdf) : rôle de mémoire de l’établissement ; aucun modèle imposé.
- [Politique de l’API OSM](https://operations.osmfoundation.org/policies/api/) : l’API d’édition n’est pas un service de fond de carte.
- [Documentation officielle Overpass](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html) : requêtes ponctuelles et limites de service ; pas de garantie de disponibilité publique.

Les deux PDF de démonstration sont locaux, sous `output/pdf`, avec des données fictives. Aucun PDF opérationnel de l’utilisateur n’est publié. La version GitHub distribuée reste 0.11.0 pendant cette recette locale.
