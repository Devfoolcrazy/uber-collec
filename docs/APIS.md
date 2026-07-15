# Les API externes, pas à pas

Uber Collec complète vos fiches (« hydratation ») en interrogeant des bases
ouvertes. Ce document décrit, source par source : à quoi elle sert, comment
s'inscrire, où mettre la clé, et ses limitations.

## Vue d'ensemble

| Source | Collections | Clé requise ? | Recherche par | Remplit |
|---|---|---|---|---|
| **BNF** | Livres, BD | non | ISBN/EAN ou titre | titre, auteurs (scénariste/dessinateur), éditeur, date, couverture |
| **Google Books** | Livres, BD | conseillée 🔑 | ISBN/EAN ou titre | synopsis, couverture d'appoint |
| **OpenLibrary** | Livres, BD | non | ISBN/EAN ou titre | couverture de secours |
| **MusicBrainz** | CD | non | code-barres ou artiste + album | titre, artiste, label, date, pochette (Cover Art Archive) |
| **Discogs** | CD | oui 🔑 | code-barres ou artiste + album | **genres**, éditions, pochettes complémentaires |
| **iTunes Search** | CD | non | code-barres ou artiste + album | pochettes haute résolution (600 px), genres |
| **TMDB** | DVD / Blu-ray | oui 🔑 | **titre uniquement** | titre, réalisateur, acteurs, genre, date, synopsis, affiche, type (film / série TV) |
| **GitHub** | sauvegarde | oui (compte) | — | versionnage de la bibliothèque, lecture iOS |

**Où saisir les clés :** barre latérale → **🔑 Clés API**. Elles sont stockées
dans la configuration locale de l'app
(`~/Library/Application Support/fr.remy.ubercollec/config.json`), **jamais**
dans la bibliothèque versionnée sur GitHub.

**Comportement commun :**

- Une chaîne de 10 ou 13 chiffres est traitée comme un code-barres (douchette
  USB : elle « tape » le code puis Entrée, aucun réglage à faire).
- Chaque requête est retentée jusqu'à 3 fois (attente croissante 2 s puis 4 s)
  sur les pannes passagères (429, 5xx, coupure réseau).
- Une source en panne n'annule pas les autres : ses résultats manquent, un
  bandeau ⚠ le signale — « introuvable » et « en panne » ne se confondent
  jamais.
- Les clés API sont masquées (`key=•••`) dans tous les messages d'erreur.
- L'enrichissement de masse marque une pause de 4 s après **chaque** requête
  réseau (~8 s par fiche) pour respecter les services publics.

---

## BNF (Bibliothèque nationale de France)

**Rôle.** Source prioritaire pour le fonds français : livres, BD, mangas.
Distingue scénariste et dessinateur, fournit les couvertures du catalogue.

**Inscription.** Aucune. L'API SRU de la BNF (`catalogue.bnf.fr`) est
publique et gratuite, sans clé ni compte.

**Limitations.** Pas de quota officiel, mais c'est un service public : l'app
s'y tient à un rythme volontairement lent en enrichissement de masse. Pas de
synopsis (c'est le rôle de Google Books). Couverture absente pour une partie
du catalogue.

---

## Google Books

**Rôle.** Synopsis (seule source de synopsis en masse pour livres/BD) et
couvertures d'appoint. Fonctionne sans clé mais s'épuise très vite (erreurs
429 « quota anonyme ») — la clé est fortement conseillée.

**Inscription, pas à pas :**

1. Allez sur [console.cloud.google.com](https://console.cloud.google.com)
   (compte Google requis).
2. Créez un projet (n'importe quel nom, ex. `uber-collec`).
3. Menu **API et services → Bibliothèque** → cherchez **Books API** →
   **Activer**.
4. Menu **API et services → Identifiants** → **Créer des identifiants →
   Clé API**. La clé ressemble à `AIza…`.
5. Recommandé : cliquez sur la clé → **Restrictions relatives aux API** →
   limitez-la à « Books API ». Ainsi, même divulguée, elle ne sert à rien
   d'autre.

**Où la mettre.** 🔑 Clés API → champ « Google Books ».

**Limitations.** **1 000 requêtes/jour** en gratuit. Un enrichissement de
masse d'une grosse collection (ex. 2 000+ BD) se fait donc sur 2-3 nuits —
l'enrichissement est rejouable, il reprend là où il s'était arrêté. Le service
alterne parfois 200 et 503 d'une seconde à l'autre ; l'app réessaie seule.

---

## OpenLibrary

**Rôle.** Filet de secours : couvertures par ISBN quand BNF et Google Books
n'en ont pas. Catalogue français faible — jamais utilisée seule.

**Inscription.** Aucune.

**Limitations.** Service communautaire (Internet Archive), parfois lent ou
indisponible ; l'app le traite comme optionnel.

---

## MusicBrainz + Cover Art Archive

**Rôle.** Référence pour les CD : recherche par code-barres ou par
artiste + album, pochettes via Cover Art Archive.

**Inscription.** Aucune.

**Limitations.** Règle de courtoisie : ~1 requête/seconde (l'app la respecte
largement). Cover Art Archive renvoie 404 quand la pochette manque — l'app
passe alors aux sources suivantes. Sans code-barres, le rapprochement
automatique n'accepte un candidat que si **artiste et titre correspondent
vraiment** (le score seul ment sur les requêtes libres).

---

## Discogs

**Rôle.** Le point fort : les **genres musicaux** (traduits en français et
ajoutés au schéma s'ils n'y sont pas), plus éditions et pochettes
complémentaires.

**Inscription, pas à pas :**

1. Créez un compte sur [discogs.com](https://www.discogs.com) (gratuit).
2. Connecté, allez dans **Settings → Developers**
   (`discogs.com/settings/developers`).
3. Cliquez **Generate new token**. C'est un « personal access token » — pas
   besoin de créer une application.
4. Copiez le token affiché.

**Où le mettre.** 🔑 Clés API → champ « Discogs ».

**Limitations.** 60 requêtes/minute avec token (25 sans) — sans objet au
rythme de l'app. Les images exigent un token : sans lui, la source est
simplement ignorée.

---

## iTunes Search

**Rôle.** Pochettes CD haute résolution (600 px) et genres, en complément.

**Inscription.** Aucune — c'est l'API publique de recherche de l'iTunes
Store.

**Limitations.** ~20 requêtes/minute recommandées (respecté d'office).
Catalogue orienté numérique : certaines éditions physiques manquent.

---

## TMDB (The Movie Database)

**Rôle.** Tout pour les DVD / Blu-ray : films **et séries TV**, affiches,
synopsis, réalisateur, acteurs, genre, et la nature de l'œuvre (champ Type :
Film / Série TV). Une série TV est « dépliée » en un candidat par saison
(« Kaamelott — Saison 2 » : jaquette, date et synopsis de la saison).

**Inscription, pas à pas :**

1. Créez un compte sur [themoviedb.org](https://www.themoviedb.org) (gratuit).
2. **Paramètres → API** (`themoviedb.org/settings/api`).
3. Demandez une clé « Developer » : usage personnel, description libre
   (ex. « gestion de ma collection de DVD »). Acceptation immédiate.
4. Deux credentials sont affichés — **les deux fonctionnent** dans l'app :
   - « Clé d'API » (v3, 32 caractères hexadécimaux) ;
   - « Jeton d'accès en lecture à l'API » (v4, long, commence par `eyJ`).

**Où la mettre.** 🔑 Clés API → champ « TMDB ». L'app la demande aussi
directement à la première recherche DVD.

**Limitations.**

- **Pas de recherche par code-barres** : TMDB ne connaît pas les EAN. La
  douchette ne sert à rien pour les DVD — recherchez par titre. L'app vous le
  dit explicitement si vous scannez.
- Quota très généreux (~50 requêtes/seconde), jamais atteint ici.
- Gratuit pour un usage personnel non commercial.
- En enrichissement de masse, le rapprochement est strict : titre exact
  (normalisé) et année à ±1 an. Une fiche au titre approximatif restera
  « non trouvée » — complétez-la individuellement (bouton « Compléter depuis
  les bases » de la fiche).

---

## GitHub (sauvegarde et iOS)

Pas une source d'hydratation, mais l'autre moitié « API » de l'app : chaque
modification de la bibliothèque est un commit poussé sur votre dépôt.

### Desktop (macOS)

1. Installez [GitHub CLI](https://cli.github.com) : `brew install gh`.
2. Authentifiez-vous **hors de l'app**, dans un terminal : `gh auth login`
   (choisir GitHub.com, HTTPS, login par navigateur).
3. Dans l'app : panneau **Synchronisation** → créer le dépôt (privé
   recommandé : vos données) ou renseigner un dépôt existant.

Ensuite tout est automatique : commit par modification, push en arrière-plan,
pull au démarrage. Le bouton « Pousser » commite d'abord les changements en
attente.

### iOS (consultation)

L'app iOS lit un instantané du dépôt via l'API GitHub (pas de git sur iOS).
Il lui faut un token à portée minimale :

1. GitHub → **Settings → Developer settings → Personal access tokens →
   Fine-grained tokens → Generate new token**.
2. **Repository access** : « Only select repositories » → votre dépôt de
   données uniquement.
3. **Permissions → Repository permissions → Contents : Read-only**. Rien
   d'autre.
4. Copiez le token (`github_pat_…`) dans l'écran de configuration de l'app
   iOS, avec le nom du dépôt (`utilisateur/depot`).

**Limitation.** Lecture seule par choix : on ne modifie pas sa collection
depuis le téléphone, on la consulte en brocante.
