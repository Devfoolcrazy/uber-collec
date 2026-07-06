# Cahier des charges — Gestionnaire de collections

**Projet** : application desktop de gestion de collections personnelles
**Utilisateur** : mono-utilisateur, macOS (mobile iOS en consultation à terme)
**Date** : 3 juillet 2026 — version 1.0 (à valider)

---

## 1. Vision

Une application desktop pour gérer des collections personnelles (~20 000 objets à terme) :
BD, livres, jeux vidéo, CD, DVD, et toute collection personnalisée définie par
l'utilisateur. Les données appartiennent à l'utilisateur : stockage en fichiers plats
lisibles (YAML), versionnés sur GitHub, sans dépendance à un service tiers.

## 2. Stack technique

| Couche | Choix | Justification |
|---|---|---|
| Cœur applicatif | **Rust + Tauri 2** | App native légère, cross-platform, cible iOS possible |
| Interface | **React + TypeScript + Vite** | Montée en compétence de l'utilisateur, écosystème riche |
| Données (référence) | **Fichiers YAML** (un fichier par objet) | Lisible, diffable, versionnable Git |
| Données (index) | **SQLite + FTS5** (jetable) | Recherche instantanée et stats sur 20 000 objets |
| Images | **WebP** local (~400 px de large) | ~25 Ko/couverture, ~500 Mo pour 20 000 objets |
| Versionnage | **Git → GitHub** (dépôt de données séparé du code) | Sauvegarde + historique gratuits |

**Principe fondamental** : les fichiers YAML sont l'unique source de vérité. L'index
SQLite est reconstructible à tout moment à partir des YAML ; sa corruption ou sa
suppression est sans conséquence.

## 3. Modèle de données

### 3.1 Structure du dépôt de données

```
ma-collection/               # dépôt Git dédié aux données
├── collections/
│   ├── bd/
│   │   ├── _schema.yaml     # définition des champs de la collection
│   │   ├── _series.yaml     # registre des séries de la collection
│   │   ├── BD-00001.yaml
│   │   └── BD-00002.yaml
│   ├── livres/
│   ├── jeux-video/
│   ├── cd/
│   ├── dvd/
│   └── vinyles/             # exemple de collection custom créée par l'utilisateur
└── images/
    └── bd/
        └── BD-00001.webp
```

### 3.2 Tout est schéma

Il n'existe **aucune différence architecturale** entre collections fixes et
personnalisées : les cinq collections fixes (BD, livres, jeux vidéo, CD, DVD) sont des
schémas pré-livrés à la création de la bibliothèque. Tous les schémas sont modifiables
(ajout/retrait de champs), y compris ceux des collections fixes.

**Types de champs génériques** (composants d'assemblage) :

| Type | Exemple d'usage |
|---|---|
| `text` | titre, éditeur |
| `text[]` | auteurs multiples |
| `number` | numéro de tome, année |
| `date` | date de parution |
| `select` | genre (liste fermée éditable) |
| `tags` | mots-clés libres multiples |
| `boolean` | case à cocher |
| `rating` | note /5 ou /10 |
| `url` | lien externe |
| `image` | couverture (stockée localement en WebP) |
| `series_ref` | référence au registre de séries + numéro de tome |

Exemple de `_schema.yaml` (extrait, collection BD) :

```yaml
name: BD
id_prefix: BD
source: bd            # adaptateur d'hydratation associé (voir §5)
fields:
  - key: titre
    type: text
    required: true
  - key: serie
    type: series_ref
  - key: scenariste
    type: text[]
  - key: dessinateur
    type: text[]
  - key: editeur
    type: text
  - key: genre
    type: select
  - key: date_parution
    type: date
  - key: ean
    type: text
  - key: couverture
    type: image
```

### 3.3 Objets

Chaque objet est un fichier YAML nommé par son ID. Champs système présents sur tout
objet, quel que soit le schéma :

- `id` — ID interne immuable (voir §4)
- `cote` — générée pour l'étiquetage (voir §4), attribuée au passage en `possede`
- `statut` — `possede` | `souhaite` (la wishlist est une vue filtrée, pas une
  collection à part ; un objet souhaité s'hydrate normalement, sa cote n'est
  générée qu'au passage en `possede`)
- `emplacement` — champ libre modifiable (`Salon/Étagère B/Rangée 3`), jamais encodé
  dans l'ID
- `date_ajout`

### 3.4 Séries

La série est une **entité à part entière**, enregistrée dans `_series.yaml` :

```yaml
- id: lastman
  nom: Lastman
  terminee: true       # coché une seule fois, au niveau série
```

Les objets y font référence (`serie: lastman`, `tome: 4`). Un one-shot est un objet
sans référence de série. Ce modèle permet :

- statut « en cours / terminée » porté par la série, pas dupliqué sur chaque album ;
- détection automatique des **tomes manquants** (je possède 1, 2, 3, 5 → il manque
  le 4) ;
- vue dashboard « séries incomplètes » et alimentation automatique de la wishlist
  (optionnelle, à la demande).

## 4. Identifiants et étiquetage

Deux notions distinctes, comme en bibliothèque :

**L'ID interne** — immuable, invisible sur l'étiquette. Sert de nom de fichier et de
clé de référence (séries, images). Format : `BD-00042` (préfixe du schéma + séquence).
Ne change jamais, quoi qu'il arrive aux données de l'objet.

**La cote** — ce qui figure sur l'étiquette de tranche. Format :
`AAAA-GENRE-NNNN` → `1986-SF-0042`

- `AAAA` : année de production de l'œuvre (parution/sortie). Si inconnue à la
  saisie : `0000`, régénérable une fois renseignée.
- `GENRE` : code court dérivé du champ genre du schéma. Chaque valeur de la liste
  de genres porte un code configurable (`Science-Fiction` → `SF`,
  `Mangas - Seinen` → `SEINEN`…), avec un défaut proposé automatiquement.
- `NNNN` : séquence par couple (année, genre) au sein de la collection.
- Pas de préfixe de collection : les collections ne sont pas mélangées physiquement.

Règles :

- La cote est **générée** à partir des données de l'objet. Si le genre ou l'année
  sont corrigés après coup, l'app régénère la cote et place l'objet dans la liste
  « étiquettes à refaire » — seule cette étiquette est à réimprimer, rien ne casse
  en interne.
- L'emplacement physique ne figure jamais dans la cote (champ séparé modifiable).
- Compacte (~12 caractères) pour tenir sur une tranche de livre/DVD.
- Pas de QR code.

**Étiquetage — contrainte matérielle** : la Dymo LetraTag est une étiqueteuse
autonome, non pilotable depuis macOS. L'application affiche donc l'ID en gros et bien
lisible sur la fiche objet (+ liste « étiquettes à faire » pour les objets récemment
ajoutés), et l'utilisateur le tape sur la LetraTag. Si un jour une Dymo LabelWriter
(connectable USB) entre en jeu, un lot d'impression directe pourra être ajouté.

## 5. Hydratation depuis les bases ouvertes

Un **adaptateur par domaine**, derrière un trait Rust commun
(`fn search(query) -> Vec<Candidate>` ; recherche par code-barres EAN/ISBN ou par
titre) :

| Domaine | Sources (ordre de priorité) |
|---|---|
| Livres / BD | OpenLibrary → Google Books → BNF (SRU) |
| Jeux vidéo | IGDB (clé Twitch gratuite) |
| CD | MusicBrainz → Discogs |
| DVD / Blu-ray | TMDB |

- Résultats présentés comme **candidats** : l'utilisateur valide/corrige avant
  écriture (mapping champs API → champs du schéma).
- Les couvertures récupérées sont téléchargées, converties en WebP ~400 px, stockées
  dans `images/`.
- La BD franco-belge est le maillon faible (pas d'API Bédéthèque) : hydratation
  partielle acceptée, complément manuel.
- Les collections custom peuvent être associées à un adaptateur existant ou à aucun.

**Saisie par douchette** : les douchettes USB fonctionnent en mode clavier HID
(compatibles macOS sans driver, ~30-60 €). L'application offre un mode « scan » :
champ toujours actif, chaque code scanné déclenche la recherche API, validation,
objet suivant. Objectif : saisie de masse rapide.

## 6. Import du CSV existant

Fichier analysé : `Collection au 03-07-2026.csv` — **2 328 BD**, colonnes : Serie,
Titre, Tome, ISBN, Genre, Scenariste, Dessinateur, Editeur, Collection,
Date parution, EAN.

- 56 % d'EAN + 525 ISBN → hydratation automatique des couvertures pour la majorité.
- 333 séries distinctes (171 multi-tomes) → création automatique du registre
  `_series.yaml` à l'import.
- Valeurs `<N/A>` à nettoyer ; auteurs au format `Nom, Prénom` à normaliser.
- L'import est un **assistant de mapping générique** (colonne CSV → champ de schéma),
  réutilisable pour tout autre import futur, avec rapport d'import (lignes OK,
  lignes à revoir).

## 7. Synchronisation Git

- Commit automatique après chaque modification (message généré :
  « Ajout BD-00042 — Lastman T4 »), push périodique ou manuel vers GitHub.
- Mono-utilisateur, mono-machine en écriture → pas de gestion de conflits nécessaire ;
  un pull au démarrage suffit.
- Configuration : URL du dépôt + authentification (token ou clé SSH existante).

## 8. Recherche, statistiques, tableaux de bord

- **Recherche** plein texte instantanée (FTS5) sur tous les champs, filtres par
  collection/genre/statut/série, tri par colonne.
- **Réponse « où est cet objet ? »** : recherche → fiche → emplacement.
- **Dashboard** : total d'objets par collection, acquisitions par an, répartition par
  genre, séries incomplètes (avec tomes manquants), taille de la wishlist.

## 9. Application mobile (scope A — consultation)

Application iOS (Tauri 2, cible iPhone 13+) en **lecture seule** : clone/pull du
dépôt GitHub de données, recherche et consultation des fiches. Cas d'usage :
« est-ce que je l'ai déjà ? » chez le bouquiniste. Aucune écriture depuis le mobile.
Livrée en dernier lot ; l'architecture (données = dépôt Git autonome, cœur Rust
partagé) la rend possible sans refonte.

## 10. Découpage en lots

| Lot | Contenu | Livrable vérifiable |
|---|---|---|
| **0 — Socle** | Scaffolding Tauri 2 + React/TS, modèle de schémas, CRUD YAML, index SQLite + reconstruction, génération d'ID interne et de cote | Créer/lire/modifier des objets via une UI minimale ; index reconstruit depuis les YAML |
| **1 — Import CSV** | Assistant de mapping, nettoyage, création du registre de séries, rapport d'import | Les 2 328 BD importées et consultables |
| **2 — UI cœur** | Vues liste/grille par collection, fiche objet, recherche + filtres, édition, emplacement, affichage ID pour étiquetage | Naviguer et rechercher dans la collection réelle |
| **3 — Hydratation** | Adaptateurs OpenLibrary/Google Books/BNF, IGDB, MusicBrainz/Discogs, TMDB ; téléchargement + conversion WebP des couvertures ; mode douchette ; enrichissement rétroactif des BD importées | Scanner un EAN → fiche pré-remplie avec couverture |
| **4 — Séries, wishlist, stats** | Gestion des séries (tomes manquants), statut possédé/souhaité, vues wishlist, dashboard | Dashboard complet sur données réelles |
| **5 — Constructeur de collections** | UI de création/édition de schémas (assemblage des types de champs génériques), édition des schémas fixes | Créer une collection « vinyles » de zéro |
| **6 — Sync Git** | Commit auto, push/pull, configuration du dépôt | Historique GitHub alimenté automatiquement |
| **7 — iOS consultation** | Build Tauri iOS, pull du dépôt, recherche/consultation lecture seule | « Est-ce que je l'ai ? » depuis l'iPhone |

Chaque lot est validé par l'utilisateur avant d'entamer le suivant. L'ordre 1 → 2
(import avant UI complète) est délibéré : développer l'interface sur les 2 328 vraies
BD plutôt que sur des données de test.

## 11. Hors scope (décisions actées)

- Multi-utilisateur, gestion des prêts
- État/condition, prix, valeur estimée, dédicaces, tirages limités (ajoutables plus
  tard via l'édition de schéma)
- QR codes, impression directe d'étiquettes (LetraTag non pilotable)
- Écriture depuis le mobile (scopes B/C écartés)
- Scraping de Bédéthèque (CGU)
