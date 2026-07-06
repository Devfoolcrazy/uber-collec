//! Lot 6 : sauvegarde Git du dépôt de données.
//!
//! On délègue au binaire `git` du système : il réutilise la configuration
//! SSH/credentials de l'utilisateur sans rien stocker de plus. Toutes les
//! opérations passent par un verrou global (un seul `git` à la fois sur le
//! dépôt), et les commits automatiques tournent en tâche de fond — jamais de
//! latence perceptible à l'enregistrement d'une fiche.

use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use tauri::Manager;

static GIT_LOCK: Mutex<()> = Mutex::new(());

/// PATH étendu : en app packagée, l'environnement GUI ne contient pas
/// Homebrew.
fn augmented_path() -> String {
    let base = std::env::var("PATH").unwrap_or_default();
    format!("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{base}")
}

fn run(root: &Path, bin: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(bin)
        .current_dir(root)
        .env("PATH", augmented_path())
        .args(args)
        .output()
        .map_err(|e| format!("{bin} : {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "{bin} {} : {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    run(root, "git", args)
}

fn is_repo(root: &Path) -> bool {
    root.join(".git").is_dir()
}

fn has_remote(root: &Path) -> bool {
    git(root, &["remote", "get-url", "origin"]).is_ok()
}

#[derive(Debug, Default, Serialize)]
pub struct SyncStatus {
    pub is_repo: bool,
    pub remote: Option<String>,
    pub dirty: bool,
    /// Commits locaux non poussés.
    pub ahead: u32,
    /// Commits distants non récupérés.
    pub behind: u32,
    pub last_commit: Option<String>,
}

pub fn status(root: &Path) -> SyncStatus {
    let _g = GIT_LOCK.lock().unwrap();
    if !is_repo(root) {
        return SyncStatus::default();
    }
    let (ahead, behind) = git(root, &["rev-list", "--count", "--left-right", "HEAD...@{u}"])
        .ok()
        .and_then(|s| {
            let mut it = s.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .unwrap_or((0, 0));
    SyncStatus {
        is_repo: true,
        remote: git(root, &["remote", "get-url", "origin"]).ok(),
        dirty: git(root, &["status", "--porcelain"])
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        ahead,
        behind,
        last_commit: git(root, &["log", "-1", "--format=%s · %cr"]).ok(),
    }
}

/// Initialise le dépôt de données : `git init`, identité locale (adresse
/// no-reply GitHub — pas d'e-mail personnel dans l'historique), premier
/// commit de l'existant.
pub fn init(root: &Path, github_login: Option<&str>) -> Result<(), String> {
    let _g = GIT_LOCK.lock().unwrap();
    if !is_repo(root) {
        git(root, &["init", "-b", "main"])?;
    }
    if let Some(login) = github_login {
        git(root, &["config", "user.email", &format!("{login}@users.noreply.github.com")])?;
        git(root, &["config", "user.name", login])?;
    }
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, ".DS_Store\n*.tmp\n").map_err(|e| e.to_string())?;
    }
    commit_all_locked(root, "Bibliothèque initiale")?;
    Ok(())
}

/// Login gh actif, si le CLI est installé et connecté.
pub fn gh_login(root: &Path) -> Option<String> {
    run(root, "gh", &["api", "user", "--jq", ".login"]).ok().filter(|s| !s.is_empty())
}

/// Crée le dépôt GitHub (privé par défaut) et pousse tout via gh CLI.
pub fn create_github_repo(root: &Path, name: &str, private: bool) -> Result<String, String> {
    let _g = GIT_LOCK.lock().unwrap();
    let visibility = if private { "--private" } else { "--public" };
    run(
        root,
        "gh",
        &["repo", "create", name, visibility, "--source", ".", "--remote", "origin", "--push"],
    )?;
    git(root, &["remote", "get-url", "origin"])
}

/// Lie un dépôt distant existant et pousse.
pub fn set_remote(root: &Path, url: &str) -> Result<(), String> {
    let _g = GIT_LOCK.lock().unwrap();
    if has_remote(root) {
        git(root, &["remote", "set-url", "origin", url])?;
    } else {
        git(root, &["remote", "add", "origin", url])?;
    }
    git(root, &["push", "-u", "origin", "main"])?;
    Ok(())
}

fn commit_all_locked(root: &Path, message: &str) -> Result<bool, String> {
    git(root, &["add", "-A"])?;
    // diff --cached --quiet sort en erreur quand il y a du contenu indexé.
    if git(root, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(false);
    }
    git(root, &["commit", "-m", message])?;
    Ok(true)
}

pub fn push(root: &Path) -> Result<(), String> {
    let _g = GIT_LOCK.lock().unwrap();
    git(root, &["push"]).map(|_| ())
}

/// Récupère les changements distants (fast-forward uniquement — mono-écrivain,
/// jamais de fusion). Renvoie true si HEAD a bougé (→ index à reconstruire).
pub fn pull(root: &Path) -> Result<bool, String> {
    let _g = GIT_LOCK.lock().unwrap();
    if !is_repo(root) || !has_remote(root) {
        return Ok(false);
    }
    let before = git(root, &["rev-parse", "HEAD"])?;
    git(root, &["pull", "--ff-only"])?;
    let after = git(root, &["rev-parse", "HEAD"])?;
    Ok(before != after)
}

/// Commit automatique en tâche de fond après une modification, suivi d'un
/// push silencieux si un remote est configuré. Les échecs de push (hors
/// ligne…) sont ignorés : le compteur « à pousser » du statut les signale.
pub fn auto_commit(app: &tauri::AppHandle, message: String) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = {
            let state = app.state::<Mutex<crate::AppState>>();
            let guard = state.lock().unwrap();
            match &guard.library {
                Some(lib) => lib.root.clone(),
                None => return,
            }
        };
        let _g = GIT_LOCK.lock().unwrap();
        if !is_repo(&root) {
            return;
        }
        match commit_all_locked(&root, &message) {
            Ok(true) if has_remote(&root) => {
                let _ = git(&root, &["push"]);
            }
            Ok(_) => {}
            Err(e) => eprintln!("auto-commit : {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_commit_and_status_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.yaml"), "titre: test\n").unwrap();
        init(root, Some("testeur")).unwrap();

        let s = status(root);
        assert!(s.is_repo);
        assert!(!s.dirty, "tout doit être commité après init");
        assert!(s.last_commit.unwrap().contains("Bibliothèque initiale"));
        assert!(s.remote.is_none());

        // Modification → dirty, puis commit → propre.
        std::fs::write(root.join("a.yaml"), "titre: modifié\n").unwrap();
        assert!(status(root).dirty);
        let changed = {
            let _g = GIT_LOCK.lock().unwrap();
            commit_all_locked(root, "Modification a.yaml").unwrap()
        };
        assert!(changed);
        assert!(!status(root).dirty);

        // Sans changement : pas de commit vide.
        let changed = {
            let _g = GIT_LOCK.lock().unwrap();
            commit_all_locked(root, "rien").unwrap()
        };
        assert!(!changed);
    }
}
