# Uber Collec

Gestionnaire de collections personnelles (BD, livres, jeux vidéo, CD, DVD… et
collections sur mesure), local d'abord : **vos données sont des fichiers YAML
lisibles, versionnés par Git, qui vous appartiennent**.

Desktop macOS (Tauri 2 + Rust + React) et consultation iOS.

## Principes

- **Fichiers plats = source de vérité.** Un objet = un fichier YAML. Un index
  SQLite FTS5 jetable accélère recherche et statistiques ; il se reconstruit à
  tout moment depuis les fichiers.
- **Tout est schéma.** Les collections fixes (BD, livres, CD, DVD, jeux vidéo)
  sont des schémas pré-livrés, modifiables comme les vôtres. Douze types de
  champs génériques s'assemblent en collections sur mesure.
- **ID immuable, cote générée.** Chaque objet a un identifiant interne stable
  (`BD-00042`) et une cote d'étiquetage régénérable (`1998-SF-0012` :
  année-genre-séquence) pour retrouver l'objet en rayon.
- **Hydratation par bases ouvertes.** BNF (prioritaire pour le fonds français,
  couvertures incluses), Google Books, OpenLibrary, MusicBrainz + Cover Art
  Archive, Discogs, TMDB. Recherche par code-barres (douchette USB en mode
  clavier) ou par titre ; enrichissement de masse à débit volontairement lent,
  avec validation stricte des rapprochements.
- **Sauvegarde Git automatique.** Chaque modification est un commit poussé sur
  votre dépôt GitHub. L'app iOS (lecture seule) consomme un instantané du même
  dépôt.

## Documentation

- **[Guide de l'application](docs/GUIDE.md)** — schémas, cotes, séries,
  recherche, imports/exports, synchronisation.
- **[Les API externes](docs/APIS.md)** — inscription pas à pas à chaque
  source (BNF, Google Books, Discogs, TMDB…), où mettre les clés,
  limitations.

## Développement

```sh
make help      # liste des commandes
make dev       # app desktop en mode développement
make test      # tests Rust + typecheck front
make install   # compile et installe dans /Applications
make ios       # simulateur iPhone
```

Prérequis : Rust, Node ≥ 20, et pour iOS : Xcode + CocoaPods.

## Clés API optionnelles

Saisies dans l'app (🔑 Clés API), stockées dans la configuration locale —
jamais dans vos données :

- **Discogs** (CD : genres, éditions) — token personnel gratuit
- **TMDB** (DVD : films, affiches, synopsis) — clé gratuite

## Licence

MIT
