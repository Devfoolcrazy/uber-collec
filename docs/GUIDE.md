# Guide de l'application

Comment Uber Collec fonctionne : les données, les schémas, les cotes, et
chaque écran. Pour les API externes (inscription, clés, limitations), voir
[APIS.md](APIS.md).

## La philosophie en trois règles

1. **Les fichiers plats sont la source de vérité.** Un objet = un fichier
   YAML lisible dans n'importe quel éditeur. La base SQLite n'est qu'un index
   jetable, reconstructible à tout moment (↻ ou ⌘R).
2. **Tout est schéma.** Les collections livrées (BD, Livres, CD, DVD, Jeux
   vidéo) sont des schémas comme les autres, modifiables ; l'interface ne
   fait qu'afficher ce que le schéma décrit. Ajoutez un champ au schéma, il
   apparaît dans le formulaire, la fiche, la recherche — et s'il est de type
   sélecteur, dans la barre de filtres.
3. **Vos données vous appartiennent.** Bibliothèque versionnée par Git sur
   votre dépôt, clés API dans la configuration locale, aucune dépendance à un
   service fermé.

## La bibliothèque sur disque

```
MaCollection/
├── collections/
│   ├── bd/
│   │   ├── _schema.yaml      # le schéma de la collection
│   │   ├── _series.yaml      # le registre des séries
│   │   ├── _counters.yaml    # compteurs d'ID et de cotes
│   │   ├── BD-00001.yaml     # une fiche = un fichier
│   │   └── …
│   └── dvd/ …
└── images/
    └── bd/BD-00001.webp      # couvertures, converties en WebP
```

Toute écriture est atomique (fichier temporaire puis renommage) : une coupure
ne corrompt jamais une fiche.

### Anatomie d'une fiche

```yaml
id: DVD-00003            # identifiant interne, IMMUABLE (nom du fichier)
cote: 1964-AVENT-0001    # cote d'étagère, régénérable
statut: possede          # ou: souhaite (= liste de souhaits)
date_ajout: 2026-07-07
titre: Goldfinger        # …puis les champs définis par le schéma
genre: Aventure
realisateur: [Guy Hamilton]
couverture: images/dvd/DVD-00003.webp
```

Six clés sont réservées et gérées par l'app : `id`, `cote`, `statut`,
`emplacement`, `etiquette`, `date_ajout`. Tout le reste vient du schéma.

## Les schémas

Éditeur : sidebar → une collection → **Schéma**. Un schéma définit le nom, le
préfixe d'ID (`BD`, `DVD`…), la source d'hydratation éventuelle, la
configuration de cote, et la liste des champs.

### Les types de champs

| Type | Usage | Exemple |
|---|---|---|
| `text` | texte court ; le premier `text` requis sert de titre | Titre, Éditeur |
| `longtext` | paragraphe | Synopsis |
| `text[]` | liste de noms — crée un onglet « personnes » | Scénaristes, Acteurs |
| `number` | nombre | Nombre de pistes |
| `date` | date (l'année alimente la cote et le filtre Année) | Date de parution |
| `select` | choix fermé, avec un **code de cote** par option ; devient un filtre déroulant | Genre, Type, Support |
| `tags` | étiquettes libres | Mots-clés |
| `boolean` | case à cocher | — |
| `rating` | note (max configurable) | Note /5 |
| `url` | lien cliquable | Fiche externe |
| `image` | couverture (WebP local ; jamais « requis ») | Jaquette |
| `series_ref` | référence au registre des séries + n° de tome | Série |

### Créer une collection sur mesure

**+ Nouvelle collection** → nom, préfixe d'ID, puis assemblez vos champs.
Choisissez éventuellement une **source d'hydratation** dans le catalogue
(livres, BD, CD, DVD) : une collection custom « LDVELH » avec la source
*livres* se scanne à la douchette comme la collection Livres. La suppression
d'une collection (bouton en bas de l'éditeur de schéma) déplace ses fichiers
hors de la bibliothèque — rien n'est détruit.

## Les cotes (étiquettes d'étagère)

Format : **`AAAA-GENRE-NNNN`** — année de l'œuvre, code du genre (défini par
option de sélecteur), séquence. Exemple : `1983-OLD-0001`.

- La config de cote d'un schéma désigne le champ année et le champ genre.
- Sans genre renseigné : code `AUTRE`. Le genre arrive plus tard (Discogs,
  correction manuelle) ? **Régénérer les cotes** (éditeur de schéma) recalcule
  celles qui ne correspondent plus, et les fiches réapparaissent dans les
  étiquettes à refaire.
- **L'ID interne ne change jamais** ; seule la cote est régénérable.

### Le panneau Étiquettes

Sidebar → **Étiquettes** (le badge indique le nombre à faire). Liste les
fiches possédées, cotées, jamais étiquetées **ou étiquetées sous une
ancienne cote** — triées par collection puis cote, l'ordre d'une session en
rayon. Tapez la cote sur la Dymo, pointez « fait » : la fiche mémorise la
cote imprimée et sort de la liste.

## Séries et liste de souhaits

- Une série vit dans le registre de sa collection : nom + case « terminée ».
  Une fiche y pointe via son champ `series_ref` avec un numéro de tome. Le
  même mécanisme couvre une saga de films (Die Hard 1-5) et une série TV
  (SG-1, saisons 1-10).
- Le **statut** de chaque fiche est `possédé` ou `souhaité` : la liste de
  souhaits n'est qu'un filtre Statut.
- Onglet **Séries** : progression tome à tome, incomplètes d'abord. Un tome
  manquant se bascule en souhait (et inversement) d'un clic sur sa pastille.
  Cliquer le nom d'une série ouvre l'onglet Objets filtré sur elle.
- Les noms de séries sont cherchés par la recherche plein texte.

## Recherche, filtres, tri

- **Recherche plein texte** : titre, auteurs, série, cote, EAN… par préfixe,
  insensible à la casse et aux accents.
- **Filtres** : Statut (tous/possédés/souhaités), Année, et un déroulant par
  champ `select` du schéma (Genre, Type, Support, Édition…), plus Série. Tous
  combinables entre eux et avec la recherche.
- **Tri** par en-tête de colonne en vue liste. Pagination par 200 avec
  « Tout afficher ».
- Vues **liste** et **grille** (couvertures).

## Ajouter et compléter des fiches

Trois chemins :

1. **Scanner** (douchette ou clavier) : scannez un code-barres ou tapez un
   titre → candidats des bases en ligne → choisir → la fiche pré-remplie
   s'ouvre. « Voir les données » détaille ce qui sera appliqué et ce que la
   source offre sans correspondance dans le schéma.
2. **+ Ajouter** : formulaire vide, saisie manuelle.
3. **Import CSV** : assistant de mappage colonnes → champs (transformation
   « Nom, Prénom → Prénom Nom » pour les listes de noms), déduplication par
   EAN ou titre+auteur+éditeur, création automatique des séries et des genres
   manquants (codes de cote uniques).

**Compléter une fiche existante** : bouton « Compléter depuis les bases »
sur la fiche — seuls les champs **vides** sont remplis, jamais d'écrasement.
La couverture se remplace aussi manuellement (choisir un fichier image,
converti en WebP).

**Enrichissement de masse** : bouton dédié de la collection. Tâche de fond
interruptible et **rejouable** : les fiches déjà complètes sont sautées sans
appel réseau, ~8 s par fiche traitée. Rapprochement strict par EAN (livres,
BD), artiste+titre (CD) ou titre+année ±1 (DVD) — le doute profite au refus,
quitte à laisser des « non trouvées » à compléter à la main.

## Exports

Bouton **Exporter** : CSV (réimportable tel quel) ou JSON, de la collection
entière **ou du résultat filtré** courant (recherche + filtres appliqués).

## Tableau de bord

Totaux possédés/souhaités par collection, répartition par genre et par
année, séries incomplètes (avec les tomes manquants).

## Synchronisation Git

Panneau **Synchronisation** : état (à pousser / à jour / en retard), création
du dépôt GitHub depuis l'app, push manuel. En routine, tout est automatique :
un commit par modification (au libellé parlant : « Ajout DVD-00042 —
Goldfinger »), push en arrière-plan, pull au démarrage. Voir
[APIS.md](APIS.md#github-sauvegarde-et-ios) pour l'installation.

L'app **iOS** est une consultation en lecture seule du même dépôt (recherche,
filtres, fiches, couvertures), rafraîchie par instantané.

## Divers

- **Thème** clair/sombre : bouton en bas de la sidebar.
- **↻ / ⌘R** : reconstruit l'index et recharge les couvertures — utile après
  une modification des fichiers hors de l'app (édition YAML à la main,
  remplacement d'une image sous le même nom).
- **Configuration locale** (chemin de bibliothèque, clés API, dépôt) :
  `~/Library/Application Support/fr.remy.ubercollec/config.json`.
- **Développement** : voir le [README](../README.md) (`make dev`, `make
  test`, `make install`…) et le [cahier des charges](../CAHIER_DES_CHARGES.md).
