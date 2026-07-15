//! Schémas pré-livrés des cinq collections fixes. Ce sont des schémas comme
//! les autres : créés une fois à l'initialisation de la bibliothèque, puis
//! librement modifiables par l'utilisateur.

/// (slug de dossier, YAML du schéma)
pub const DEFAULT_SCHEMAS: &[(&str, &str)] = &[
    ("bd", BD), ("livres", LIVRES), ("jeux-video", JEUX_VIDEO), ("cd", CD), ("dvd", DVD),
];

const BD: &str = r#"
name: BD
id_prefix: BD
source: bd
cote:
  year_field: date_parution
  genre_field: genre
fields:
  - { key: titre, label: Titre, type: text, required: true }
  - { key: serie, label: Série, type: series_ref }
  - { key: scenariste, label: Scénariste(s), type: "text[]" }
  - { key: dessinateur, label: Dessinateur(s), type: "text[]" }
  - { key: editeur, label: Éditeur, type: text }
  - { key: collection_editeur, label: Collection éditeur, type: text }
  - key: genre
    label: Genre
    type: select
    options:
      - { value: Science-Fiction, code: SF }
      - { value: Fantasy, code: FANT }
      - { value: Historique, code: HIST }
      - { value: Humour, code: HUM }
      - { value: Aventures Humoristiques, code: AVHUM }
      - { value: Aventure, code: AVENT }
      - { value: Polar, code: POLAR }
      - { value: Romans Graphiques, code: GRAPH }
      - { value: Comics, code: COMIC }
      - { value: Comics Super-héros, code: SUPER }
      - { value: Mangas - Seinen, code: SEINEN }
      - { value: Mangas - Shônen, code: SHONEN }
      - { value: Mangas - Shôjo, code: SHOJO }
      - { value: Jeunesse, code: JEUN }
      - { value: Érotique, code: ERO }
      - { value: Périodique, code: PERIO }
  - { key: date_parution, label: Date de parution, type: date }
  - { key: synopsis, label: Synopsis, type: longtext }
  - { key: ean, label: EAN / ISBN, type: text }
  - { key: couverture, label: Couverture, type: image }
"#;

const LIVRES: &str = r#"
name: Livres
id_prefix: LIV
source: livres
cote:
  year_field: date_parution
  genre_field: genre
fields:
  - { key: titre, label: Titre, type: text, required: true }
  - { key: serie, label: Série / Cycle, type: series_ref }
  - { key: auteur, label: Auteur(s), type: "text[]" }
  - { key: editeur, label: Éditeur, type: text }
  - { key: collection_editeur, label: Collection éditeur, type: text }
  - key: genre
    label: Genre
    type: select
    options:
      - { value: Roman, code: ROMAN }
      - { value: Science-Fiction, code: SF }
      - { value: Fantasy, code: FANT }
      - { value: Policier, code: POLAR }
      - { value: Essai, code: ESSAI }
      - { value: Biographie, code: BIO }
      - { value: Histoire, code: HIST }
      - { value: Poésie, code: POESIE }
      - { value: Théâtre, code: THEA }
      - { value: Jeunesse, code: JEUN }
  - { key: date_parution, label: Date de parution, type: date }
  - { key: synopsis, label: Synopsis, type: longtext }
  - { key: isbn, label: ISBN, type: text }
  - { key: couverture, label: Couverture, type: image }
"#;

const JEUX_VIDEO: &str = r#"
name: Jeux vidéo
id_prefix: JV
source: jeux-video
cote:
  year_field: date_sortie
  genre_field: genre
fields:
  - { key: titre, label: Titre, type: text, required: true }
  - { key: serie, label: Série / Saga, type: series_ref }
  - key: plateforme
    label: Plateforme
    type: select
    options:
      - { value: PC, code: PC }
      - { value: PlayStation 5, code: PS5 }
      - { value: PlayStation 4, code: PS4 }
      - { value: Nintendo Switch, code: NSW }
      - { value: Xbox Series, code: XBS }
      - { value: Xbox One, code: XBO }
      - { value: Rétro, code: RETRO }
  - key: genre
    label: Genre
    type: select
    options:
      - { value: Action, code: ACT }
      - { value: RPG, code: RPG }
      - { value: FPS, code: FPS }
      - { value: Aventure, code: AVENT }
      - { value: Plateforme, code: PLAT }
      - { value: Stratégie, code: STRAT }
      - { value: Course, code: RACE }
      - { value: Sport, code: SPORT }
      - { value: Puzzle, code: PUZZ }
      - { value: Simulation, code: SIM }
  - { key: developpeur, label: Développeur, type: text }
  - { key: editeur, label: Éditeur, type: text }
  - { key: date_sortie, label: Date de sortie, type: date }
  - { key: ean, label: Code-barres (EAN), type: text }
  - { key: couverture, label: Jaquette, type: image }
"#;

const CD: &str = r#"
name: CD audio
id_prefix: CD
source: cd
cote:
  year_field: date_sortie
  genre_field: genre
fields:
  - { key: titre, label: Titre de l'album, type: text, required: true }
  - { key: artiste, label: Artiste(s), type: "text[]" }
  - key: genre
    label: Genre
    type: select
    options:
      - { value: Rock, code: ROCK }
      - { value: Pop, code: POP }
      - { value: Jazz, code: JAZZ }
      - { value: Classique, code: CLASS }
      - { value: Rap / Hip-hop, code: RAP }
      - { value: Metal, code: METAL }
      - { value: Électro, code: ELEC }
      - { value: Chanson française, code: CHANS }
      - { value: Blues, code: BLUES }
      - { value: Folk, code: FOLK }
      - { value: Bande originale, code: BO }
  - { key: label, label: Label, type: text }
  - { key: date_sortie, label: Date de sortie, type: date }
  - { key: pistes, label: Nombre de pistes, type: number }
  - { key: ean, label: Code-barres (EAN), type: text }
  - { key: couverture, label: Pochette, type: image }
"#;

const DVD: &str = r#"
name: DVD / Blu-ray
id_prefix: DVD
source: dvd
cote:
  year_field: date_sortie
  genre_field: genre
fields:
  - { key: titre, label: Titre, type: text, required: true }
  - { key: serie, label: Saga / Série, type: series_ref }
  - { key: realisateur, label: Réalisateur(s), type: "text[]" }
  - { key: acteurs, label: Acteurs principaux, type: "text[]" }
  - key: genre
    label: Genre
    type: select
    options:
      - { value: Science-Fiction, code: SF }
      - { value: Comédie, code: COM }
      - { value: Drame, code: DRAME }
      - { value: Action, code: ACT }
      - { value: Thriller, code: THRIL }
      - { value: Horreur, code: HORR }
      - { value: Animation, code: ANIM }
      - { value: Aventure, code: AVENT }
      - { value: Fantastique, code: FANT }
      - { value: Documentaire, code: DOCU }
  - key: type
    label: Type
    type: select
    options:
      - { value: Film, code: FILM }
      - { value: Série TV, code: SERIE }
  - key: support
    label: Support
    type: select
    options:
      - { value: DVD, code: DVD }
      - { value: Blu-ray, code: BR }
      - { value: Blu-ray 4K, code: BR4K }
  - { key: studio, label: Studio, type: text }
  - { key: date_sortie, label: Date de sortie, type: date }
  - { key: synopsis, label: Synopsis, type: longtext }
  - { key: ean, label: Code-barres (EAN), type: text }
  - { key: couverture, label: Jaquette, type: image }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Schema;

    #[test]
    fn all_default_schemas_parse() {
        for (slug, yaml) in DEFAULT_SCHEMAS {
            let schema: Schema = serde_yaml::from_str(yaml)
                .unwrap_or_else(|e| panic!("schéma {slug} invalide: {e}"));
            assert!(!schema.fields.is_empty(), "{slug} sans champs");
            assert!(schema.title_field().is_some(), "{slug} sans champ titre");
            let cote = schema.cote.as_ref().expect("cote config");
            assert!(schema.field(&cote.year_field).is_some(), "{slug}: year_field inconnu");
            assert!(schema.field(&cote.genre_field).is_some(), "{slug}: genre_field inconnu");
        }
    }
}
