//! Lot 7 : alimentation de l'app iOS (consultation seule).
//!
//! Pas de git sur iOS : la bibliothèque arrive sous forme d'instantané du
//! dépôt GitHub (API tarball), extrait dans le bac à sable de l'app. Le
//! rafraîchissement remplace l'instantané de façon atomique puis reconstruit
//! l'index — le même moteur (YAML + SQLite FTS5) tourne tel quel sur iOS.

use flate2::read::GzDecoder;
use std::path::Path;
use tar::Archive;

/// Télécharge `owner/repo` (branche par défaut) et remplace `dest`.
pub async fn fetch_snapshot(repo: &str, token: &str, dest: &Path) -> Result<(), String> {
    eprintln!("mobile_sync : téléchargement de {repo}…");
    let url = format!("https://api.github.com/repos/{repo}/tarball/HEAD");
    let client = reqwest::Client::builder()
        .user_agent("UberCollec/0.1")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .bearer_auth(token.trim())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("téléchargement GitHub : {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("dépôt introuvable — vérifiez « propriétaire/nom » et les droits du token".into());
    }
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("token refusé par GitHub — vérifiez qu'il a l'accès en lecture au dépôt".into());
    }
    let resp = resp.error_for_status().map_err(|e| format!("GitHub : {e}"))?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    eprintln!("mobile_sync : {} octets reçus, extraction…", bytes.len());

    let dest = dest.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || extract_snapshot(&bytes, &dest))
        .await
        .map_err(|e| e.to_string())?
}

/// Extraction dans un dossier temporaire, puis bascule atomique : jamais de
/// bibliothèque à moitié écrite, même si le réseau ou l'app tombe.
fn extract_snapshot(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let staging = dest.with_extension("staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    Archive::new(GzDecoder::new(bytes))
        .unpack(&staging)
        .map_err(|e| format!("extraction : {e}"))?;

    // Le tarball GitHub contient un unique dossier racine « owner-repo-sha ».
    let inner = std::fs::read_dir(&staging)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .ok_or("archive GitHub vide")?;
    if !inner.join("collections").is_dir() {
        std::fs::remove_dir_all(&staging).ok();
        return Err("ce dépôt ne contient pas une bibliothèque (dossier collections/ absent)".into());
    }

    let old = dest.with_extension("old");
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        std::fs::rename(dest, &old).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&inner, dest).map_err(|e| e.to_string())?;
    std::fs::remove_dir_all(&staging).ok();
    std::fs::remove_dir_all(&old).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Fabrique un tarball façon GitHub (dossier racine owner-repo-sha).
    fn fake_tarball() -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        let content = b"name: BD\nid_prefix: BD\nfields:\n- {key: titre, label: Titre, type: text, required: true}\n";
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "owner-repo-abc123/collections/bd/_schema.yaml",
                &content[..],
            )
            .unwrap();
        let gz = builder.into_inner().unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn extract_replaces_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("biblio");

        extract_snapshot(&fake_tarball(), &dest).unwrap();
        assert!(dest.join("collections/bd/_schema.yaml").is_file());

        // Un ancien contenu est remplacé, pas fusionné.
        std::fs::File::create(dest.join("obsolete.txt"))
            .unwrap()
            .write_all(b"x")
            .unwrap();
        extract_snapshot(&fake_tarball(), &dest).unwrap();
        assert!(dest.join("collections/bd/_schema.yaml").is_file());
        assert!(!dest.join("obsolete.txt").exists());
    }

    #[test]
    fn rejects_non_library_archive() {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(2);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "owner-repo-abc/readme.md", &b"hi"[..])
            .unwrap();
        let gz = builder.into_inner().unwrap();
        let bytes = gz.finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let err = extract_snapshot(&bytes, &dir.path().join("b")).unwrap_err();
        assert!(err.contains("collections"), "{err}");
    }
}
