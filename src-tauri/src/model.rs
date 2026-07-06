use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Types de champs génériques assemblables dans un schéma de collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Longtext,
    #[serde(rename = "text[]")]
    TextList,
    Number,
    Date,
    Select,
    Tags,
    Boolean,
    Rating,
    Url,
    Image,
    SeriesRef,
}

/// Valeur possible d'un champ `select`, avec son code court utilisé dans la cote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl SelectOption {
    /// Code effectif : celui défini, sinon dérivé de la valeur.
    pub fn effective_code(&self) -> String {
        self.code
            .clone()
            .unwrap_or_else(|| derive_code(&self.value))
    }
}

/// Dérive un code court (≤ 6 caractères alphanumériques majuscules) d'un libellé.
pub fn derive_code(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(unaccent)
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    if cleaned.is_empty() {
        "AUTRE".to_string()
    } else {
        cleaned.chars().take(6).collect()
    }
}

pub(crate) fn unaccent(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' | 'á' | 'À' | 'Â' | 'Ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'î' | 'ï' | 'Î' | 'Ï' => 'i',
        'ô' | 'ö' | 'Ô' | 'Ö' => 'o',
        'ù' | 'û' | 'ü' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        other => other,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub required: bool,
    /// Valeurs autorisées (champs `select` uniquement).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Note maximale (champs `rating` uniquement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u8>,
}

/// Champs sources de la cote d'étiquetage `AAAA-GENRE-NNNN`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoteConfig {
    pub year_field: String,
    pub genre_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    pub id_prefix: String,
    /// Adaptateur d'hydratation associé (lot 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cote: Option<CoteConfig>,
    pub fields: Vec<FieldDef>,
}

/// Clés système interdites comme clés de champ.
pub const RESERVED_KEYS: &[&str] = &["id", "cote", "statut", "emplacement", "date_ajout"];

impl Schema {
    pub fn field(&self, key: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.key == key)
    }

    /// Garde-fous avant écriture d'un schéma (création ou édition).
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("le nom de la collection est obligatoire".into());
        }
        if self.id_prefix.trim().is_empty()
            || !self.id_prefix.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err("le préfixe d'ID doit être alphanumérique (ex : VIN)".into());
        }
        if self.fields.is_empty() {
            return Err("au moins un champ est nécessaire".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for f in &self.fields {
            if f.key.is_empty()
                || !f
                    .key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                return Err(format!(
                    "clé de champ invalide « {} » (minuscules, chiffres, _)",
                    f.key
                ));
            }
            if RESERVED_KEYS.contains(&f.key.as_str()) {
                return Err(format!("« {} » est une clé système réservée", f.key));
            }
            if !seen.insert(&f.key) {
                return Err(format!("clé de champ en double : « {} »", f.key));
            }
        }
        if self.title_field().is_none() {
            return Err("il faut au moins un champ de type texte (le titre)".into());
        }
        if self.fields.iter().filter(|f| f.field_type == FieldType::SeriesRef).count() > 1 {
            return Err("une collection ne peut avoir qu'un champ série".into());
        }
        if let Some(cote) = &self.cote {
            match self.field(&cote.year_field) {
                Some(f) if f.field_type == FieldType::Date => {}
                _ => return Err("le champ année de la cote doit être un champ date".into()),
            }
            match self.field(&cote.genre_field) {
                Some(f) if f.field_type == FieldType::Select => {}
                _ => return Err("le champ genre de la cote doit être une liste à choix".into()),
            }
        }
        Ok(())
    }

    /// Premier champ texte requis, utilisé comme libellé de l'objet dans les listes.
    pub fn title_field(&self) -> Option<&FieldDef> {
        self.fields
            .iter()
            .find(|f| f.field_type == FieldType::Text && f.required)
            .or_else(|| {
                self.fields
                    .iter()
                    .find(|f| f.field_type == FieldType::Text)
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Statut {
    Possede,
    Souhaite,
}

/// Un objet de collection. Les champs définis par le schéma sont aplatis
/// au même niveau que les champs système dans le YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cote: Option<String>,
    pub statut: Statut,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emplacement: Option<String>,
    pub date_ajout: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: String,
    pub nom: String,
    #[serde(default)]
    pub terminee: bool,
}

/// Compteurs persistés par collection (`_counters.yaml`) : séquence des ID
/// internes et séquences de cotes par couple (année, genre). Versionnés avec
/// les données, reconstructibles par balayage des fichiers objets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Counters {
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub cotes: BTreeMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_code_strips_accents_and_truncates() {
        assert_eq!(derive_code("Science-Fiction"), "SCIENC");
        assert_eq!(derive_code("Érotique"), "EROTIQ");
        assert_eq!(derive_code("SF"), "SF");
        assert_eq!(derive_code("---"), "AUTRE");
    }

    #[test]
    fn schema_validation_guards() {
        let mut schema: Schema = serde_yaml::from_str(
            "name: Vinyles\nid_prefix: VIN\nfields:\n  - { key: titre, label: Titre, type: text, required: true }\n",
        )
        .unwrap();
        assert!(schema.validate().is_ok());

        schema.fields.push(FieldDef {
            key: "statut".into(),
            label: "Statut".into(),
            field_type: FieldType::Text,
            required: false,
            options: vec![],
            max: None,
        });
        assert!(schema.validate().unwrap_err().contains("réservée"));
        schema.fields.pop();

        schema.cote = Some(CoteConfig {
            year_field: "titre".into(),
            genre_field: "titre".into(),
        });
        assert!(schema.validate().unwrap_err().contains("date"));
    }

    #[test]
    fn item_yaml_roundtrip_flattens_schema_fields() {
        let yaml = "id: BD-00001\nstatut: possede\ndate_ajout: '2026-07-03'\ntitre: Lastman\ntome: 4\n";
        let item: Item = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(item.id, "BD-00001");
        assert_eq!(item.fields["titre"], serde_json::json!("Lastman"));
        assert_eq!(item.fields["tome"], serde_json::json!(4));
        let back = serde_yaml::to_string(&item).unwrap();
        assert!(back.contains("titre: Lastman"));
        assert!(!back.contains("cote:"));
    }
}
