mod db;
mod git;
mod repo_meta;

use base64::Engine;
use db::{Database, RepoRecord};
use tauri::{Emitter, Manager};
use git::{
    clone_repo, discover_repos_under, head_sha, latest_commit_at, last_commits_for_paths,
    last_commit_for_path, list_refs, list_remotes, log_oneline_for_rev, resolve_git_binary,
    resolve_repo, rev_list_count, rev_parse_verify, show_blob, show_commit_patch, CommitSummary,
    GitError, RefLists, RemoteInfo, StatusLine, TreeEntry,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HubRefreshHeadsSummary {
    total: usize,
    ok: usize,
    failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseAsset {
    id: String,
    name: String,
    stored_path: String,
    original_path: String,
    size_bytes: u64,
    added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseEntry {
    id: String,
    version: String,
    title: String,
    notes: String,
    source_url: String,
    assets: Vec<ReleaseAsset>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoPathCommit {
    path: String,
    commit: Option<CommitSummary>,
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn sanitize_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed
    }
}

fn repo_releases_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".deskvio").join("releases")
}

fn repo_releases_json_path(repo_root: &Path) -> PathBuf {
    repo_releases_meta_dir(repo_root).join("releases.json")
}

fn normalize_stored_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn is_safe_asset_stored_path(stored_path: &str) -> bool {
    let normalized = normalize_stored_path(stored_path);
    if !normalized.starts_with("assets/") {
        return false;
    }
    let p = Path::new(&normalized);
    if p.is_absolute() {
        return false;
    }
    !p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

fn resolve_asset_disk_path(repo_root: &Path, stored_path: &str) -> Result<PathBuf, String> {
    if !is_safe_asset_stored_path(stored_path) {
        return Err("invalid stored asset path".to_string());
    }
    Ok(repo_releases_meta_dir(repo_root)
        .join(normalize_stored_path(stored_path).replace('/', std::path::MAIN_SEPARATOR_STR)))
}

fn make_unique_asset_target(
    repo_root: &Path,
    release_id: &str,
    original_file_name: &str,
) -> (String, String, PathBuf) {
    let release_seg = sanitize_segment(release_id);
    let safe_name = sanitize_segment(original_file_name);
    let mut asset_id = Uuid::new_v4().simple().to_string();
    let mut rel_path = format!("assets/{}/{}-{}", release_seg, asset_id, safe_name);
    let mut dst = repo_releases_meta_dir(repo_root)
        .join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let mut n = 2usize;
    while dst.exists() {
        asset_id = format!("{}-{}", Uuid::new_v4().simple(), n);
        rel_path = format!("assets/{}/{}-{}", release_seg, asset_id, safe_name);
        dst = repo_releases_meta_dir(repo_root)
            .join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        n += 1;
    }
    (asset_id, rel_path, dst)
}

fn read_releases_file(repo_root: &Path) -> Result<Vec<ReleaseEntry>, String> {
    let path = repo_releases_json_path(repo_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str::<Vec<ReleaseEntry>>(&text).map_err(|e| e.to_string())
}

fn write_releases_file(repo_root: &Path, entries: &[ReleaseEntry]) -> Result<(), String> {
    let dir = repo_releases_meta_dir(repo_root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = repo_releases_json_path(repo_root);
    let text = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension(format!("json.tmp.{}", Uuid::new_v4().simple()));
    fs::write(&tmp_path, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, &path).map_err(|e| e.to_string())
}

fn cleanup_removed_assets(
    repo_root: &Path,
    previous: &[ReleaseEntry],
    next: &[ReleaseEntry],
) -> Result<(), String> {
    let previous_paths: std::collections::HashSet<String> = previous
        .iter()
        .flat_map(|r| r.assets.iter().map(|a| normalize_stored_path(&a.stored_path)))
        .collect();
    let next_paths: std::collections::HashSet<String> = next
        .iter()
        .flat_map(|r| r.assets.iter().map(|a| normalize_stored_path(&a.stored_path)))
        .collect();
    for removed in previous_paths.difference(&next_paths) {
        if !is_safe_asset_stored_path(removed) {
            continue;
        }
        if let Ok(path) = resolve_asset_disk_path(repo_root, removed) {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn assert_unique_release_versions(entries: &[ReleaseEntry]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let ver = entry.version.trim();
        if ver.is_empty() {
            continue;
        }
        let key = ver.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(format!("duplicate release version: {}", ver));
        }
    }
    Ok(())
}
fn app_data_root() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("com", "wildchiken", "Deskvio")
        .map(|p| p.data_local_dir().to_path_buf())
        .ok_or_else(|| "could not resolve application data directory".to_string())
}

fn best_effort_make_dir_writable(dir: &Path) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("chflags")
            .args(["-R", "nouchg", dir.to_string_lossy().as_ref()])
            .output();
        let _ = std::process::Command::new("chmod")
            .args(["-R", "u+rwX", dir.to_string_lossy().as_ref()])
            .output();
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

fn open_db_with_repair(db_path: &Path, app_dir: &Path) -> Result<(Database, DbStatusInfo), String> {
    if let Ok(db) = Database::open(db_path) {
        return Ok((
            db,
            DbStatusInfo {
                status: DbRepairStatus::Ok,
                db_path: db_path.to_string_lossy().to_string(),
            },
        ));
    }

    best_effort_make_dir_writable(app_dir);

    if db_path.exists() {
        let file_name = db_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "hub.db".to_string());
        let backup = db_path.with_file_name(format!("{}.bak.{}", file_name, Uuid::new_v4().simple()));
        let _ = std::fs::rename(db_path, backup);
    }

    match Database::open(db_path) {
        Ok(db) => Ok((
            db,
            DbStatusInfo {
                status: DbRepairStatus::Repaired,
                db_path: db_path.to_string_lossy().to_string(),
            },
        )),
        Err(e) => {
            let tmp_root = std::env::temp_dir()
                .join("deskvio")
                .join(format!("db-{}", Uuid::new_v4().simple()));
            std::fs::create_dir_all(&tmp_root).map_err(|_| e.to_string())?;
            let tmp_db = tmp_root.join("hub.db");
            let db = Database::open(&tmp_db).map_err(|_| e.to_string())?;
            Ok((
                db,
                DbStatusInfo {
                    status: DbRepairStatus::Temp,
                    db_path: tmp_db.to_string_lossy().to_string(),
                },
            ))
        }
    }
}

fn legacy_joined_app_data_root() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("com", "wildchiken", "Deskvio")
        .map(|p| p.data_local_dir().join("deskvio"))
        .ok_or_else(|| "could not resolve application data directory".to_string())
}

fn migrate_legacy_joined_app_data_root(new_root: &Path) -> Result<(), String> {
    let old_root = legacy_joined_app_data_root()?;
    if old_root == new_root {
        return Ok(());
    }

    let new_db = new_root.join("hub.db");
    if new_db.exists() {
        return Ok(());
    }

    let old_db = old_root.join("hub.db");
    if !old_db.exists() {
        return Ok(());
    }

    if !new_root.exists() {
        if let Some(parent) = new_root.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        match std::fs::rename(&old_root, new_root) {
            Ok(()) => return Ok(()),
            Err(_) => {
                std::fs::create_dir_all(new_root).map_err(|e| e.to_string())?;
            }
        }
    }

    let new_db_path = new_root.join("hub.db");
    if !new_db_path.exists() {
        std::fs::rename(&old_db, &new_db_path).or_else(|_| {
            std::fs::copy(&old_db, &new_db_path).map_err(|e| e.to_string())?;
            std::fs::remove_file(&old_db).map_err(|e| e.to_string())
        }).map_err(|e| e.to_string())?;
    }

    let old_repos = old_root.join("repositories");
    let new_repos = new_root.join("repositories");
    if old_repos.exists() && !new_repos.exists() {
        if let Err(_) = std::fs::rename(&old_repos, &new_repos) {
            std::fs::create_dir_all(&new_repos).map_err(|e| e.to_string())?;
            for entry in std::fs::read_dir(&old_repos).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let src = entry.path();
                let dst = new_repos.join(entry.file_name());
                if src.is_dir() {
                    std::fs::rename(&src, &dst).map_err(|e| e.to_string())?;
                } else {
                    std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
                }
            }
        }
    }

    Ok(())
}

fn legacy_app_data_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let candidates = [
        ("com", "wildchiken", "ProjectShelf", "projectshelf"),
        ("com", "atlas", "OfflineGitHub", "offline-git-hub"),
    ];
    for (qualifier, organization, application, leaf) in candidates {
        if let Some(p) = directories::ProjectDirs::from(qualifier, organization, application) {
            roots.push(p.data_local_dir().join(leaf));
        }
    }
    roots
}

fn migrate_legacy_app_data(new_root: &Path) -> Result<(), String> {
    let new_db = new_root.join("hub.db");
    if new_db.exists() {
        return Ok(());
    }
    let marker = new_root.join(".migrated-from-legacy");
    if marker.exists() {
        return Ok(());
    }
    for old_root in legacy_app_data_roots() {
        if old_root == new_root {
            continue;
        }
        let old_db = old_root.join("hub.db");
        if !old_db.exists() {
            continue;
        }
        std::fs::create_dir_all(new_root).map_err(|e| e.to_string())?;
        std::fs::copy(&old_db, &new_db).map_err(|e| e.to_string())?;
        std::fs::write(&marker, old_root.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        break;
    }
    Ok(())
}

const MAX_ZIP_ENTRIES: usize = 20_000;
const MAX_ZIP_TOTAL_UNPACKED_BYTES: u64 = 1_000_000_000;
const MAX_ZIP_SINGLE_FILE_BYTES: u64 = 256_000_000;
const MAX_ZIP_PATH_DEPTH: usize = 32;

fn zip_path_depth(path: &Path) -> usize {
    path.components().count()
}

fn looks_like_git_repo_dir(path: &Path) -> bool {
    let dot_git = path.join(".git");
    dot_git.is_dir()
        || dot_git.is_file()
        || (path.join("HEAD").is_file() && path.join("objects").is_dir())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
enum DbRepairStatus {
    Ok,
    Repaired,
    Temp,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DbStatusInfo {
    status: DbRepairStatus,
    db_path: String,
}

pub struct AppState {
    db: Mutex<Database>,
    db_status: DbStatusInfo,
    git_bin: PathBuf,
    clone_sessions: Mutex<std::collections::HashMap<String, CloneSession>>,
}

#[derive(Clone)]
struct CloneSession {
    pid: u32,
    cancelled: bool,
    target: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelCloneResult {
    session_id: String,
    target: String,
    killed: bool,
    removed: bool,
    still_exists: bool,
    error: Option<String>,
}

fn best_effort_remove_dir_all(path: &Path) {
    if !path.exists() {
        return;
    }
    if !path.is_absolute() {
        return;
    }
    if path.parent().is_none() {
        return;
    }

    let max_attempts = 60;
    let mut last_err: Option<String> = None;
    for attempt in 0..max_attempts {
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("chflags")
                .args(["-R", "nouchg", path.to_string_lossy().as_ref()])
                .output();
            let _ = std::process::Command::new("chmod")
                .args(["-R", "u+rwX", path.to_string_lossy().as_ref()])
                .output();
        }

        match fs::remove_dir_all(path) {
            Ok(_) => {
                println!(
                    "[clone-cleanup] removed '{}' after {} attempts",
                    path.display(),
                    attempt + 1
                );
                return;
            }
            Err(e) => {
                if !path.exists() {
                    println!(
                        "[clone-cleanup] target '{}' disappeared (attempt {})",
                        path.display(),
                        attempt + 1
                    );
                    return;
                }
                last_err = Some(e.to_string());
                if attempt + 1 < max_attempts {
                    let ms = (120u64 * (attempt as u64 + 1)).min(1500);
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                } else {
                }
            }
        }
    }

    if path.exists() {
        println!(
            "[clone-cleanup] failed to remove '{}' after {} attempts: {}",
            path.display(),
            max_attempts,
            last_err.unwrap_or_else(|| "unknown error".to_string())
        );
    } else {
        println!(
            "[clone-cleanup] removed '{}' (exists check said missing after retries)",
            path.display()
        );
    }
}

fn map_git_err(e: GitError) -> String {
    e.to_string()
}

fn map_db_err(e: rusqlite::Error) -> String {
    e.to_string()
}

fn scan_directory_inner(
    state: &AppState,
    root: PathBuf,
    max_depth: usize,
) -> Result<Vec<RepoRecord>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let discovered = discover_repos_under(&root, max_depth);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut added = Vec::new();
    for path in discovered {
        let ctx = match resolve_repo(&state.git_bin, &path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let head = head_sha(&state.git_bin, &ctx).ok();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string());
        let mut rec = db
            .insert_repo(
                &path.to_string_lossy(),
                name,
                ctx.bare,
                head,
            )
            .map_err(map_db_err)?;
        let _ = repo_meta::merge_disk_into_db(&db, rec.id, &path);
        if let Ok(Some(updated)) = db.get(rec.id) {
            rec = updated;
        }
        added.push(rec);
    }
    Ok(added)
}

#[tauri::command]
fn hub_list_repos(state: tauri::State<'_, AppState>) -> Result<Vec<RepoRecord>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_all().map_err(map_db_err)
}

#[tauri::command]
fn app_db_status(state: tauri::State<'_, AppState>) -> Result<DbStatusInfo, String> {
    Ok(state.db_status.clone())
}

#[tauri::command]
fn hub_prune_missing_repos(state: tauri::State<'_, AppState>) -> Result<u64, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let repos = db.list_all().map_err(map_db_err)?;
    let mut pruned: u64 = 0;
    for r in repos {
        let p = PathBuf::from(&r.path);
        if !p.exists() {
            if db
                .delete(r.id)
                .map_err(map_db_err)?
            {
                pruned += 1;
            }
        }
    }
    Ok(pruned)
}

#[tauri::command]
fn hub_search(state: tauri::State<'_, AppState>, query: String) -> Result<Vec<RepoRecord>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    if query.trim().is_empty() {
        return db.list_all().map_err(map_db_err);
    }
    db.search(query.trim()).map_err(map_db_err)
}

#[tauri::command]
fn hub_add_repo(state: tauri::State<'_, AppState>, path: String) -> Result<RepoRecord, String> {
    let p = PathBuf::from(path.trim());
    let canon = p.canonicalize().map_err(|e| e.to_string())?;
    let ctx = resolve_repo(&state.git_bin, &canon).map_err(map_git_err)?;
    let head = head_sha(&state.git_bin, &ctx).ok();
    let name = canon
        .file_name()
        .map(|s| s.to_string_lossy().to_string());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut rec = db
        .insert_repo(
            &canon.to_string_lossy(),
            name,
            ctx.bare,
            head.clone(),
        )
        .map_err(map_db_err)?;
    let _ = repo_meta::merge_disk_into_db(&db, rec.id, &canon);
    if let Ok(Some(updated)) = db.get(rec.id) {
        rec = updated;
    }
    Ok(rec)
}

#[tauri::command]
fn hub_default_repo_root() -> Result<String, String> {
    Err("repo root is not set; please choose a destination directory first".to_string())
}

#[tauri::command]
fn hub_legacy_repo_root() -> Result<String, String> {
    Err("repo root is not set; please choose a destination directory first".to_string())
}

#[tauri::command]
fn hub_clone_repo(
    state: tauri::State<'_, AppState>,
    url: String,
    dest_parent: Option<String>,
) -> Result<RepoRecord, String> {
    let url = url.trim();
    if !url.starts_with("https://") {
        return Err("only public https clone URLs are supported".into());
    }
    let base = match dest_parent {
        Some(dest) => {
            let trimmed = dest.trim();
            if trimmed.is_empty() {
                return Err("repo root is not set; please choose a destination directory first".to_string());
            }
            PathBuf::from(trimmed)
        }
        None => {
            return Err("repo root is not set; please choose a destination directory first".to_string());
        }
    };
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    let mut name = url
        .rsplit('/')
        .next()
        .unwrap_or("repo")
        .trim()
        .trim_end_matches(".git")
        .to_string();
    if name.is_empty() {
        name = "repo".to_string();
    }
    let mut target = base.join(&name);
    let mut idx = 2usize;
    while target.exists() {
        target = base.join(format!("{}-{}", name, idx));
        idx += 1;
    }

    clone_repo(&state.git_bin, url, &target).map_err(map_git_err)?;
    let canon = target.canonicalize().map_err(|e| e.to_string())?;
    let ctx = resolve_repo(&state.git_bin, &canon).map_err(map_git_err)?;
    let head = head_sha(&state.git_bin, &ctx).ok();
    let display_name = canon
        .file_name()
        .map(|s| s.to_string_lossy().to_string());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut rec = db
        .insert_repo(
            &canon.to_string_lossy(),
            display_name,
            ctx.bare,
            head,
        )
        .map_err(map_db_err)?;
    let _ = repo_meta::merge_disk_into_db(&db, rec.id, &canon);
    if let Ok(Some(updated)) = db.get(rec.id) {
        rec = updated;
    }
    Ok(rec)
}

fn compute_clone_dest(url: &str, dest_parent: Option<String>) -> Result<PathBuf, String> {
    let base = match dest_parent {
        Some(dest) => {
            let trimmed = dest.trim();
            if trimmed.is_empty() {
                return Err("repo root is not set; please choose a destination directory first".to_string());
            }
            PathBuf::from(trimmed)
        }
        None => {
            return Err("repo root is not set; please choose a destination directory first".to_string());
        }
    };
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let mut name = url.rsplit('/').next().unwrap_or("repo").trim().trim_end_matches(".git").to_string();
    if name.is_empty() { name = "repo".to_string(); }
    let mut target = base.join(&name);
    let mut idx = 2usize;
    while target.exists() {
        target = base.join(format!("{}-{}", name, idx));
        idx += 1;
    }
    Ok(target)
}

#[tauri::command]
fn hub_clone_repo_stream(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    url: String,
    dest_parent: Option<String>,
) -> Result<String, String> {
    let url = url.trim().to_string();
    if !url.starts_with("https://") {
        return Err("only public https clone URLs are supported".into());
    }
    let target = compute_clone_dest(&url, dest_parent)?;
    let git_bin = state.git_bin.clone();
    let session_id = Uuid::new_v4().simple().to_string();

    let child = std::process::Command::new(&git_bin)
        .args(["clone", "--progress", "--", &url, &target.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn git: {}", e))?;

    let pid = child.id();
    {
        let mut sessions = state
            .clone_sessions
            .lock()
            .map_err(|e| e.to_string())?;
        sessions.insert(
            session_id.clone(),
            CloneSession {
                pid,
                cancelled: false,
                target: target.clone(),
            },
        );
    }

    let sid = session_id.clone();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut child = child;
        if let Some(stderr) = child.stderr.take() {
            let mut reader = std::io::BufReader::new(stderr);
            let mut buf = [0u8; 4096];
            let mut line_buf = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        for &b in &buf[..n] {
                            if b == b'\r' || b == b'\n' {
                                if !line_buf.is_empty() {
                                    let line = String::from_utf8_lossy(&line_buf).to_string();
                                    let _ = app.emit("clone-progress", serde_json::json!({
                                        "sessionId": &sid,
                                        "line": line,
                                    }));
                                    line_buf.clear();
                                }
                            } else {
                                line_buf.push(b);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if !line_buf.is_empty() {
                let line = String::from_utf8_lossy(&line_buf).to_string();
                let _ = app.emit("clone-progress", serde_json::json!({
                    "sessionId": &sid,
                    "line": line,
                }));
            }
        }
        let status = child.wait();
        let mut session_was_present = false;
        let mut was_cancelled = false;
        if let Ok(mut sessions) = app.state::<AppState>().clone_sessions.lock() {
            if let Some(s) = sessions.remove(&sid) {
                session_was_present = true;
                was_cancelled = s.cancelled;
            }
        }
        match status {
            Ok(s) if s.success() => {
                if was_cancelled {
                    best_effort_remove_dir_all(&target);
                    let _ = app.emit("clone-done", serde_json::json!({
                        "sessionId": &sid,
                        "ok": false,
                        "error": "cancelled",
                    }));
                    return;
                }
                let reg = (|| -> Result<RepoRecord, String> {
                    let st = app.state::<AppState>();
                    let canon = target.canonicalize().map_err(|e| e.to_string())?;
                    let ctx = resolve_repo(&st.git_bin, &canon).map_err(map_git_err)?;
                    let head = head_sha(&st.git_bin, &ctx).ok();
                    let display_name = canon.file_name().map(|s| s.to_string_lossy().to_string());
                    let db = st.db.lock().map_err(|e| e.to_string())?;
                    let mut rec = db
                        .insert_repo(&canon.to_string_lossy(), display_name, ctx.bare, head)
                        .map_err(map_db_err)?;
                    let _ = repo_meta::merge_disk_into_db(&db, rec.id, &canon);
                    if let Ok(Some(updated)) = db.get(rec.id) {
                        rec = updated;
                    }
                    Ok(rec)
                })();
                let _ = app.emit("clone-done", serde_json::json!({
                    "sessionId": &sid,
                    "ok": true,
                    "error": null,
                }));
                let _ = reg;
            }
            Ok(s) => {
                let _ = app.emit("clone-done", serde_json::json!({
                    "sessionId": &sid,
                    "ok": false,
                    "error": format!("git clone exited with {}", s),
                }));
                if session_was_present {
                    best_effort_remove_dir_all(&target);
                }
            }
            Err(e) => {
                let _ = app.emit("clone-done", serde_json::json!({
                    "sessionId": &sid,
                    "ok": false,
                    "error": e.to_string(),
                }));
                if session_was_present {
                    best_effort_remove_dir_all(&target);
                }
            }
        }
    });

    Ok(session_id)
}

fn kill_process_by_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
}

#[tauri::command]
fn hub_cancel_clone(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<CancelCloneResult, String> {
    let pid_opt = {
        let mut sessions = match state.clone_sessions.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(s) = sessions.get_mut(&session_id) {
            s.cancelled = true;
            let pid = s.pid;
            let target = s.target.clone();
            Some((pid, target))
        } else {
            None
        }
    };

    let mut killed = false;
    let mut target_opt: Option<PathBuf> = None;
    if let Some((pid, target)) = pid_opt {
        killed = true;
        kill_process_by_pid(pid);
        best_effort_remove_dir_all(&target);
        target_opt = Some(target);
    }

    let (target_str, still_exists) = if let Some(target) = target_opt {
        let target_str = target.to_string_lossy().to_string();
        let mut still_exists = target.exists();
        if still_exists {
            for _ in 0..6 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if !target.exists() {
                    still_exists = false;
                    break;
                }
            }
        }
        (target_str, still_exists)
    } else {
        ("".to_string(), false)
    };

    let removed = if target_str.is_empty() { false } else { !still_exists };
    let error = if target_str.is_empty() {
        Some("clone session not found".to_string())
    } else {
        None
    };

    Ok(CancelCloneResult {
        session_id,
        target: target_str,
        killed,
        removed,
        still_exists,
        error,
    })
}

fn normalize_clone_url(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

#[tauri::command]
fn hub_check_clone_conflict(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<Option<RepoRecord>, String> {
    let norm_input = normalize_clone_url(url.trim());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let repos = db.list_all().map_err(map_db_err)?;
    drop(db);
    for repo in repos {
        if !PathBuf::from(&repo.path).exists() {
            continue;
        }
        let Ok(ctx) = load_ctx(&state.git_bin, &repo.path) else {
            continue;
        };
        let Ok(remotes) = list_remotes(&state.git_bin, &ctx) else {
            continue;
        };
        for remote in &remotes {
            if remote.name == "origin" {
                if normalize_clone_url(&remote.fetch_url) == norm_input {
                    return Ok(Some(repo));
                }
            }
        }
    }
    Ok(None)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FetchResetResult {
    ok: bool,
    dirty: bool,
    stashed: bool,
    error: Option<String>,
}

#[tauri::command]
fn hub_overwrite_fetch_reset(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<FetchResetResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let repo = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);

    let path_str = repo.path.clone();

    let status_out = std::process::Command::new(&state.git_bin)
        .args(["-C", &path_str, "status", "--porcelain"])
        .output()
        .map_err(|e| e.to_string())?;

    if !status_out.status.success() {
        let err = String::from_utf8_lossy(&status_out.stderr).trim().to_string();
        return Ok(FetchResetResult { ok: false, dirty: false, stashed: false, error: Some(err) });
    }

    let status_text = String::from_utf8_lossy(&status_out.stdout);
    if !status_text.trim().is_empty() {
        let mut has_tracked_uncommitted = false;
        for line in status_text.lines() {
            let t = line.trim_start();
            if t.is_empty() {
                continue;
            }
            if t.starts_with("??") {
                continue;
            }
            if t.starts_with("!!") {
                continue;
            }
            has_tracked_uncommitted = true;
            break;
        }
        if has_tracked_uncommitted {
            return Ok(FetchResetResult {
                ok: false,
                dirty: true,
                stashed: false,
                error: None,
            });
        }
    }

    let fetch_out = std::process::Command::new(&state.git_bin)
        .args(["-C", &path_str, "fetch", "origin"])
        .output()
        .map_err(|e| e.to_string())?;

    if !fetch_out.status.success() {
        let err = String::from_utf8_lossy(&fetch_out.stderr).trim().to_string();
        return Ok(FetchResetResult {
            ok: false,
            dirty: false,
            stashed: false,
            error: Some(format!("fetch failed: {}", err)),
        });
    }

    let reset_out = std::process::Command::new(&state.git_bin)
        .args(["-C", &path_str, "reset", "--hard", "FETCH_HEAD"])
        .output()
        .map_err(|e| e.to_string())?;

    if !reset_out.status.success() {
        let err = String::from_utf8_lossy(&reset_out.stderr).trim().to_string();
        return Ok(FetchResetResult {
            ok: false,
            dirty: false,
            stashed: false,
            error: Some(format!("reset failed: {}", err)),
        });
    }

    if let Ok(ctx) = load_ctx(&state.git_bin, &path_str) {
        let new_head = head_sha(&state.git_bin, &ctx).ok();
        if let Ok(db) = state.db.lock() {
            let _ = db.insert_repo(&path_str, repo.display_name, repo.is_bare, new_head);
        }
    }

    Ok(FetchResetResult { ok: true, dirty: false, stashed: false, error: None })
}

#[tauri::command]
fn hub_overwrite_fetch_reset_auto(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<FetchResetResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let repo = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);

    let path_str = repo.path.clone();

    let status_out = std::process::Command::new(&state.git_bin)
        .args(["-C", &path_str, "status", "--porcelain"])
        .output()
        .map_err(|e| e.to_string())?;

    if !status_out.status.success() {
        let err = String::from_utf8_lossy(&status_out.stderr).trim().to_string();
        return Ok(FetchResetResult {
            ok: false,
            dirty: false,
            stashed: false,
            error: Some(err),
        });
    }

    let status_text = String::from_utf8_lossy(&status_out.stdout);
    let mut has_tracked_uncommitted = false;
    if !status_text.trim().is_empty() {
        for line in status_text.lines() {
            let t = line.trim_start();
            if t.is_empty() {
                continue;
            }
            if t.starts_with("??") {
                continue;
            }
            if t.starts_with("!!") {
                continue;
            }
            has_tracked_uncommitted = true;
            break;
        }
    }

    if has_tracked_uncommitted {
        let ts = now_unix();
        let stash_msg = format!("deskvio-overwrite-{}", ts);
        let stash_out = std::process::Command::new(&state.git_bin)
            .args(["-C", &path_str, "stash", "push", "-u", "-m", &stash_msg])
            .output()
            .map_err(|e| e.to_string())?;
        if !stash_out.status.success() {
            let err = String::from_utf8_lossy(&stash_out.stderr).trim().to_string();
            return Ok(FetchResetResult {
                ok: false,
                dirty: false,
                stashed: false,
                error: Some(format!("stash failed: {}", err)),
            });
        }
    }

    let fetch_out = std::process::Command::new(&state.git_bin)
        .args(["-C", &path_str, "fetch", "origin"])
        .output()
        .map_err(|e| e.to_string())?;

    if !fetch_out.status.success() {
        let err = String::from_utf8_lossy(&fetch_out.stderr).trim().to_string();
        return Ok(FetchResetResult {
            ok: false,
            dirty: false,
            stashed: false,
            error: Some(format!("fetch failed: {}", err)),
        });
    }

    let reset_out = std::process::Command::new(&state.git_bin)
        .args(["-C", &path_str, "reset", "--hard", "FETCH_HEAD"])
        .output()
        .map_err(|e| e.to_string())?;

    if !reset_out.status.success() {
        let err = String::from_utf8_lossy(&reset_out.stderr).trim().to_string();
        return Ok(FetchResetResult {
            ok: false,
            dirty: false,
            stashed: false,
            error: Some(format!("reset failed: {}", err)),
        });
    }

    if let Ok(ctx) = load_ctx(&state.git_bin, &path_str) {
        let new_head = head_sha(&state.git_bin, &ctx).ok();
        if let Ok(db) = state.db.lock() {
            let _ = db.insert_repo(&path_str, repo.display_name, repo.is_bare, new_head);
        }
    }

    Ok(FetchResetResult {
        ok: true,
        dirty: false,
        stashed: has_tracked_uncommitted,
        error: None,
    })
}

#[tauri::command]
fn hub_remove_repo(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let rec = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    let repo_path = PathBuf::from(&rec.path);
    if !repo_path.is_absolute() {
        return Err("refusing to delete non-absolute path".into());
    }
    if repo_path.parent().is_none() {
        return Err("refusing to delete filesystem root".into());
    }
    let canon = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.clone());
    if canon.exists() && !looks_like_git_repo_dir(&canon) {
        return Err("refusing to delete path that does not look like a git repository".into());
    }
    if canon.exists() {
        fs::remove_dir_all(&canon)
            .map_err(|e| format!("failed to delete repository directory: {}", e))?;
    }
    db.delete(id).map_err(map_db_err)?;
    Ok(())
}

#[tauri::command]
fn hub_unlink_repo(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.delete(id).map_err(map_db_err)?;
    Ok(())
}

#[tauri::command]
fn hub_scan_directory(
    state: tauri::State<'_, AppState>,
    root: String,
    max_depth: Option<usize>,
) -> Result<Vec<RepoRecord>, String> {
    let root = PathBuf::from(root.trim());
    let depth = max_depth.unwrap_or(12);
    scan_directory_inner(&*state, root, depth)
}

#[tauri::command]
fn hub_set_favorite(
    state: tauri::State<'_, AppState>,
    id: i64,
    favorite: bool,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_favorite(id, favorite).map_err(map_db_err)
}

#[tauri::command]
fn hub_set_tags(
    state: tauri::State<'_, AppState>,
    id: i64,
    tags: Vec<String>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_tags(id, &tags).map_err(map_db_err)?;
    repo_meta::persist_from_db(&db, id)?;
    Ok(())
}

#[tauri::command]
fn hub_set_display_name(
    state: tauri::State<'_, AppState>,
    id: i64,
    name: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_display_name(id, name).map_err(map_db_err)
}

#[tauri::command]
fn hub_set_project_intro(
    state: tauri::State<'_, AppState>,
    id: i64,
    intro: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_project_intro(id, intro).map_err(map_db_err)?;
    repo_meta::persist_from_db(&db, id)?;
    Ok(())
}

#[tauri::command]
fn hub_sync_repo_meta_from_disk(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<RepoRecord, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    let root = Path::new(&r.path);
    repo_meta::merge_disk_into_db(&db, id, root).map_err(map_db_err)?;
    db.get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())
}

#[tauri::command]
fn repo_resolve_worktree_path(
    state: tauri::State<'_, AppState>,
    id: i64,
    relative_path: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    if r.is_bare {
        return Err("bare repository has no worktree files".into());
    }
    let path = repo_meta::resolve_worktree_file_path(&r.path, relative_path.trim())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn hub_touch_repo(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.touch_opened(id).map_err(map_db_err)
}

#[tauri::command]
fn hub_refresh_heads(state: tauri::State<'_, AppState>) -> Result<HubRefreshHeadsSummary, String> {
    let pairs: Vec<(i64, String)> = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.list_all()
            .map_err(map_db_err)?
            .into_iter()
            .map(|r| (r.id, r.path))
            .collect()
    };
    let total = pairs.len();
    let mut ok = 0usize;
    let mut failed = 0usize;
    for (id, path) in pairs {
        let (head, got) = match PathBuf::from(&path).canonicalize() {
            Ok(canon) => match resolve_repo(&state.git_bin, &canon) {
                Ok(ctx) => match head_sha(&state.git_bin, &ctx) {
                    Ok(h) => (Some(h), true),
                    Err(_) => (None, false),
                },
                Err(_) => (None, false),
            },
            Err(_) => (None, false),
        };
        if got {
            ok += 1;
        } else {
            failed += 1;
        }
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.update_cached_head(id, head.as_deref()).map_err(map_db_err)?;
    }
    Ok(HubRefreshHeadsSummary { total, ok, failed })
}

fn load_ctx(git: &Path, path: &str) -> Result<git::RepoContext, String> {
    let p = PathBuf::from(path);
    let canon = p.canonicalize().map_err(|e| e.to_string())?;
    resolve_repo(git, &canon).map_err(map_git_err)
}

#[tauri::command]
fn repo_list_releases(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Vec<ReleaseEntry>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let repo_root = PathBuf::from(r.path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    read_releases_file(&repo_root)
}

#[tauri::command]
fn repo_save_releases(
    state: tauri::State<'_, AppState>,
    id: i64,
    releases: Vec<ReleaseEntry>,
) -> Result<Vec<ReleaseEntry>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let repo_root = PathBuf::from(r.path)
        .canonicalize()
        .map_err(|e| e.to_string())?;

    let existing = read_releases_file(&repo_root).unwrap_or_default();
    let mut normalized: Vec<ReleaseEntry> = releases
        .into_iter()
        .filter_map(|mut x| {
            x.id = sanitize_segment(&x.id);
            x.version = x.version.trim().to_string();
            x.title = x.title.trim().to_string();
            x.notes = x.notes.trim().to_string();
            x.source_url = x.source_url.trim().to_string();
            x.assets = x
                .assets
                .into_iter()
                .map(|mut a| {
                    a.id = sanitize_segment(&a.id);
                    a.name = a.name.trim().to_string();
                    a.stored_path = normalize_stored_path(&a.stored_path);
                    a.original_path = a.original_path.trim().to_string();
                    a
                })
                .collect();
            if x.id.is_empty() || x.version.is_empty() {
                None
            } else {
                Some(x)
            }
        })
        .collect();

    assert_unique_release_versions(&normalized)?;
    normalized.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    write_releases_file(&repo_root, &normalized)?;
    if let Err(e) = cleanup_removed_assets(&repo_root, &existing, &normalized) {
        eprintln!("warning: release asset cleanup failed: {}", e);
    }
    Ok(normalized)
}

fn import_release_file_at(
    repo_root: &Path,
    release_id: &str,
    src_can: &Path,
    display_name: String,
) -> Result<ReleaseAsset, String> {
    if !src_can.is_file() {
        return Err("source file not found".into());
    }
    let leaf = src_can
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "asset.bin".to_string());
    let (asset_id, rel_path, dst) = make_unique_asset_target(repo_root, release_id, &leaf);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(src_can, &dst).map_err(|e| e.to_string())?;
    let meta = fs::metadata(&dst).map_err(|e| e.to_string())?;
    Ok(ReleaseAsset {
        id: asset_id,
        name: display_name,
        stored_path: rel_path,
        original_path: src_can.to_string_lossy().to_string(),
        size_bytes: meta.len(),
        added_at: now_unix(),
    })
}

#[tauri::command]
fn repo_import_release_asset(
    state: tauri::State<'_, AppState>,
    id: i64,
    release_id: String,
    source_path: String,
) -> Result<ReleaseAsset, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let repo_root = PathBuf::from(r.path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let src = PathBuf::from(source_path.trim());
    if !src.is_file() {
        return Err("source file not found".into());
    }
    let src_can = src.canonicalize().map_err(|e| e.to_string())?;
    let display_name = src_can
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "asset.bin".to_string());
    import_release_file_at(&repo_root, &release_id, &src_can, display_name)
}

#[tauri::command]
fn repo_import_release_sources(
    state: tauri::State<'_, AppState>,
    id: i64,
    release_id: String,
    source_paths: Vec<String>,
) -> Result<Vec<ReleaseAsset>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let repo_root = PathBuf::from(r.path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut all = Vec::new();
    for raw in source_paths {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let p = PathBuf::from(raw);
        if !p.exists() {
            return Err(format!("path not found: {}", raw));
        }
        if p.is_dir() {
            return Err("folders are not supported; select files only".into());
        } else if p.is_file() {
            let src_can = p.canonicalize().map_err(|e| e.to_string())?;
            let display_name = src_can
                .file_name()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| "asset.bin".to_string());
            all.push(import_release_file_at(
                &repo_root,
                &release_id,
                &src_can,
                display_name,
            )?);
        } else {
            return Err(format!("not a file or directory: {}", raw));
        }
    }
    Ok(all)
}

#[tauri::command]
fn repo_delete_release_asset(
    state: tauri::State<'_, AppState>,
    id: i64,
    stored_path: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let repo_root = PathBuf::from(r.path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = resolve_asset_disk_path(&repo_root, &stored_path)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn repo_resolve_release_asset_path(
    state: tauri::State<'_, AppState>,
    id: i64,
    stored_path: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let repo_root = PathBuf::from(r.path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = resolve_asset_disk_path(&repo_root, &stored_path)?;
    if !path.is_file() {
        return Err("asset file not found".into());
    }
    Ok(path.to_string_lossy().to_string())
}

fn repo_ls_tree_impl(state: &AppState, id: i64, rev: Option<String>) -> Result<Vec<TreeEntry>, String> {
    let treeish = rev.unwrap_or_else(|| "HEAD".into());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let current = rev_parse_verify(&state.git_bin, &ctx, &treeish).map_err(map_git_err)?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    if let Some(entries) = db
        .tree_cache_get_if_current(id, &treeish, &current)
        .map_err(map_db_err)?
    {
        drop(db);
        return Ok(entries);
    }
    drop(db);
    let entries = git::ls_tree(&state.git_bin, &ctx, &treeish).map_err(map_git_err)?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let _ = db.tree_cache_put(id, &treeish, &current, &entries);
    drop(db);
    Ok(entries)
}

#[tauri::command]
fn repo_ls_tree(
    state: tauri::State<'_, AppState>,
    id: i64,
    rev: Option<String>,
) -> Result<Vec<TreeEntry>, String> {
    repo_ls_tree_impl(&state, id, rev)
}

#[tauri::command]
fn repo_warm_tree_cache(
    state: tauri::State<'_, AppState>,
    id: i64,
    rev: Option<String>,
) -> Result<(), String> {
    let _ = repo_ls_tree_impl(&state, id, rev)?;
    Ok(())
}

#[tauri::command]
fn repo_log(
    state: tauri::State<'_, AppState>,
    id: i64,
    limit: Option<usize>,
    rev: Option<String>,
) -> Result<Vec<CommitSummary>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let treeish = rev.unwrap_or_else(|| "HEAD".into());
    log_oneline_for_rev(
        &state.git_bin,
        &ctx,
        &treeish,
        limit.unwrap_or(80),
    )
    .map_err(map_git_err)
}

#[tauri::command]
fn repo_list_refs(state: tauri::State<'_, AppState>, id: i64) -> Result<RefLists, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    list_refs(&state.git_bin, &ctx, 400).map_err(map_git_err)
}

#[tauri::command]
fn repo_latest_commit(
    state: tauri::State<'_, AppState>,
    id: i64,
    rev: String,
) -> Result<Option<CommitSummary>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    latest_commit_at(&state.git_bin, &ctx, rev.trim()).map_err(map_git_err)
}

#[tauri::command]
fn repo_rev_count(
    state: tauri::State<'_, AppState>,
    id: i64,
    rev: String,
) -> Result<usize, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    rev_list_count(&state.git_bin, &ctx, rev.trim()).map_err(map_git_err)
}

#[tauri::command]
fn repo_path_last_commit(
    state: tauri::State<'_, AppState>,
    id: i64,
    rev: String,
    path: String,
) -> Result<Option<CommitSummary>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    last_commit_for_path(&state.git_bin, &ctx, rev.trim(), path.trim()).map_err(map_git_err)
}

#[tauri::command]
fn repo_paths_last_commit(
    state: tauri::State<'_, AppState>,
    id: i64,
    rev: String,
    paths: Vec<String>,
) -> Result<Vec<RepoPathCommit>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let rev = rev.trim().to_string();
    let paths: Vec<String> = paths
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let pairs = last_commits_for_paths(&state.git_bin, &ctx, &rev, &paths).map_err(map_git_err)?;
    Ok(pairs
        .into_iter()
        .map(|(path, commit)| RepoPathCommit { path, commit })
        .collect())
}

#[tauri::command]
fn repo_remotes(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<RemoteInfo>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    list_remotes(&state.git_bin, &ctx).map_err(map_git_err)
}

#[tauri::command]
fn repo_show_commit(
    state: tauri::State<'_, AppState>,
    id: i64,
    commit: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    show_commit_patch(&state.git_bin, &ctx, &commit).map_err(map_git_err)
}

#[tauri::command]
fn repo_blob_text(
    state: tauri::State<'_, AppState>,
    id: i64,
    spec: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let bytes = show_blob(&state.git_bin, &ctx, &spec).map_err(map_git_err)?;
    String::from_utf8(bytes).map_err(|e| format!("binary or non-UTF-8 file: {}", e))
}

#[tauri::command]
fn repo_blob_base64(
    state: tauri::State<'_, AppState>,
    id: i64,
    spec: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let bytes = show_blob(&state.git_bin, &ctx, &spec).map_err(map_git_err)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[tauri::command]
fn repo_status(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<StatusLine>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let list = git::status_porcelain(&state.git_bin, &ctx).map_err(map_git_err)?;
    let head = head_sha(&state.git_bin, &ctx).ok();
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let _ = db.update_cached_head(id, head.as_deref());
    Ok(list)
}

#[tauri::command]
fn repo_stage(
    state: tauri::State<'_, AppState>,
    id: i64,
    paths: Vec<String>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    git::add_paths(&state.git_bin, &ctx, &paths).map_err(map_git_err)
}

#[tauri::command]
fn repo_commit(
    state: tauri::State<'_, AppState>,
    id: i64,
    message: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let out = git::commit_message(&state.git_bin, &ctx, &message).map_err(map_git_err)?;
    if let Ok(h) = head_sha(&state.git_bin, &ctx) {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.update_cached_head(id, Some(&h));
        let _ = db.tree_cache_invalidate_repo(id);
    }
    Ok(out)
}

#[tauri::command]
fn import_zip(
    state: tauri::State<'_, AppState>,
    zip_path: String,
    dest_parent: Option<String>,
) -> Result<Vec<RepoRecord>, String> {
    use std::fs;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    let zip_path = PathBuf::from(zip_path.trim());
    if !zip_path.is_file() {
        return Err("zip file not found".into());
    }

    let base_dest = match dest_parent {
        Some(p) => {
            let trimmed = p.trim();
            if trimmed.is_empty() {
                return Err("repo root is not set; please choose a destination directory first".to_string());
            }
            let dir = PathBuf::from(trimmed);
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            dir.join(format!("import-{}", ts))
        }
        None => {
            return Err("repo root is not set; please choose a destination directory first".to_string());
        }
    };

    fs::create_dir_all(&base_dest).map_err(|e| e.to_string())?;

    let data = fs::read(&zip_path).map_err(|e| e.to_string())?;
    let reader = Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("invalid zip: {}", e))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(format!(
            "zip contains too many entries: {} (max {})",
            archive.len(),
            MAX_ZIP_ENTRIES
        ));
    }
    let mut total_unpacked: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let enclosed = match file.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        if zip_path_depth(&enclosed) > MAX_ZIP_PATH_DEPTH {
            return Err(format!(
                "zip entry path too deep: {} (max depth {})",
                enclosed.display(),
                MAX_ZIP_PATH_DEPTH
            ));
        }
        let declared_size = file.size();
        if declared_size > MAX_ZIP_SINGLE_FILE_BYTES {
            return Err(format!(
                "zip entry too large: {} bytes (max {})",
                declared_size, MAX_ZIP_SINGLE_FILE_BYTES
            ));
        }
        total_unpacked = total_unpacked
            .checked_add(declared_size)
            .ok_or_else(|| "zip unpack size overflow".to_string())?;
        if total_unpacked > MAX_ZIP_TOTAL_UNPACKED_BYTES {
            return Err(format!(
                "zip unpack size exceeds limit: {} bytes (max {})",
                total_unpacked, MAX_ZIP_TOTAL_UNPACKED_BYTES
            ));
        }
        let outpath = base_dest.join(enclosed);
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut outfile = fs::File::create(&outpath).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }

    scan_directory_inner(&*state, base_dest, 20)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn zip_depth_counts_components() {
        assert_eq!(zip_path_depth(Path::new("a/b/c")), 3);
        assert_eq!(zip_path_depth(Path::new("readme.md")), 1);
    }

    #[test]
    fn open_db_with_repair_returns_ok_when_db_missing() {
        let tmp_root = std::env::temp_dir().join(format!("deskvio-db-ok-{}", Uuid::new_v4()));
        let app_dir = tmp_root.join("app");
        fs::create_dir_all(&app_dir).expect("mkdir app_dir");
        let db_path = app_dir.join("hub.db");
        assert!(!db_path.exists());

        let (_db, st) = open_db_with_repair(&db_path, &app_dir).expect("open db");
        assert!(matches!(st.status, DbRepairStatus::Ok));
        assert_eq!(st.db_path, db_path.to_string_lossy().to_string());

        let db2 = Database::open(&db_path).expect("db open after repair");
        assert!(db2.list_all().expect("list").is_empty());

        let _ = fs::remove_dir_all(&tmp_root);
    }

    #[test]
    fn open_db_with_repair_returns_repaired_when_db_corrupt() {
        let tmp_root = std::env::temp_dir().join(format!(
            "deskvio-db-corrupt-{}",
            Uuid::new_v4()
        ));
        let app_dir = tmp_root.join("app");
        fs::create_dir_all(&app_dir).expect("mkdir app_dir");
        let db_path = app_dir.join("hub.db");

        let mut f = fs::File::create(&db_path).expect("create corrupt db");
        f.write_all(b"not a sqlite database").expect("write corrupt db");

        let (_db, st) = open_db_with_repair(&db_path, &app_dir).expect("open db");
        assert!(matches!(st.status, DbRepairStatus::Repaired));

        let mut has_backup = false;
        for entry in fs::read_dir(&app_dir).expect("read dir") {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(".bak.") {
                has_backup = true;
            }
        }
        assert!(has_backup, "expected a .bak. backup file");

        let db2 = Database::open(&db_path).expect("db open after repair");
        let _ = db2.list_all().expect("list");

        let _ = fs::remove_dir_all(&tmp_root);
    }

    #[test]
    fn unique_versions_are_enforced_case_insensitive() {
        let now = now_unix();
        let rel = |id: &str, version: &str| ReleaseEntry {
            id: id.to_string(),
            version: version.to_string(),
            title: String::new(),
            notes: String::new(),
            source_url: String::new(),
            assets: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        let dup = vec![rel("a", "v1.0.0"), rel("b", "V1.0.0")];
        assert!(assert_unique_release_versions(&dup).is_err());
    }

    #[test]
    fn import_target_avoids_existing_collisions() {
        let temp_root = std::env::temp_dir().join(format!("ogh-test-{}", Uuid::new_v4()));
        let meta_dir = repo_releases_meta_dir(&temp_root);
        fs::create_dir_all(&meta_dir).expect("mkdir");
        let (_, first_rel, first_dst) = make_unique_asset_target(&temp_root, "r1", "archive.zip");
        if let Some(parent) = first_dst.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::write(&first_dst, b"x").expect("seed file");
        let (_, second_rel, second_dst) = make_unique_asset_target(&temp_root, "r1", "archive.zip");
        assert_ne!(first_rel, second_rel);
        assert_ne!(first_dst, second_dst);
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn release_file_roundtrip_is_consistent() {
        let temp_root = std::env::temp_dir().join(format!("ogh-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_root).expect("mkdir");
        let now = now_unix();
        let entries = vec![ReleaseEntry {
            id: "rel1".into(),
            version: "v1.2.3".into(),
            title: "Release 1.2.3".into(),
            notes: "notes".into(),
            source_url: "".into(),
            assets: vec![ReleaseAsset {
                id: "a1".into(),
                name: "archive.zip".into(),
                stored_path: "assets/rel1/a1-archive.zip".into(),
                original_path: "/tmp/archive.zip".into(),
                size_bytes: 123,
                added_at: now,
            }],
            created_at: now,
            updated_at: now,
        }];
        write_releases_file(&temp_root, &entries).expect("write releases");
        let loaded = read_releases_file(&temp_root).expect("read releases");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].version, "v1.2.3");
        assert_eq!(loaded[0].assets.len(), 1);
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn removed_assets_are_deleted_from_disk() {
        let temp_root = std::env::temp_dir().join(format!("ogh-test-{}", Uuid::new_v4()));
        fs::create_dir_all(repo_releases_meta_dir(&temp_root)).expect("mkdir");
        let asset_path = resolve_asset_disk_path(&temp_root, "assets/rel1/a1-file.bin")
            .expect("asset path");
        if let Some(parent) = asset_path.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::write(&asset_path, b"asset").expect("write asset");
        let now = now_unix();
        let previous = vec![ReleaseEntry {
            id: "rel1".into(),
            version: "v1".into(),
            title: String::new(),
            notes: String::new(),
            source_url: String::new(),
            assets: vec![ReleaseAsset {
                id: "a1".into(),
                name: "file.bin".into(),
                stored_path: "assets/rel1/a1-file.bin".into(),
                original_path: String::new(),
                size_bytes: 5,
                added_at: now,
            }],
            created_at: now,
            updated_at: now,
        }];
        let next = vec![ReleaseEntry {
            assets: Vec::new(),
            ..previous[0].clone()
        }];
        cleanup_removed_assets(&temp_root, &previous, &next).expect("cleanup");
        assert!(!asset_path.exists());
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn write_overwrites_corrupted_json_consistently() {
        let temp_root = std::env::temp_dir().join(format!("ogh-test-{}", Uuid::new_v4()));
        fs::create_dir_all(repo_releases_meta_dir(&temp_root)).expect("mkdir");
        let path = repo_releases_json_path(&temp_root);
        fs::write(&path, "{bad").expect("corrupt json");
        assert!(read_releases_file(&temp_root).is_err());

        let now = now_unix();
        let entries = vec![ReleaseEntry {
            id: "rel1".into(),
            version: "v1.0.0".into(),
            title: String::new(),
            notes: String::new(),
            source_url: String::new(),
            assets: Vec::new(),
            created_at: now,
            updated_at: now,
        }];
        write_releases_file(&temp_root, &entries).expect("rewrite releases");
        let loaded = read_releases_file(&temp_root).expect("load rewritten");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].version, "v1.0.0");
        let _ = fs::remove_dir_all(temp_root);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_dir = app_data_root()?;
            migrate_legacy_joined_app_data_root(&app_dir)?;
            std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
            migrate_legacy_app_data(&app_dir)?;
            let db_path = app_dir.join("hub.db");
            let (db, db_status) = open_db_with_repair(&db_path, &app_dir)?;
            let git_bin = resolve_git_binary();
            app.manage(AppState {
                db: Mutex::new(db),
                db_status,
                git_bin,
                clone_sessions: Mutex::new(std::collections::HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            hub_list_repos,
            app_db_status,
            hub_prune_missing_repos,
            hub_search,
            hub_add_repo,
            hub_default_repo_root,
            hub_legacy_repo_root,
            hub_clone_repo,
            hub_clone_repo_stream,
            hub_cancel_clone,
            hub_check_clone_conflict,
            hub_overwrite_fetch_reset,
            hub_overwrite_fetch_reset_auto,
            hub_remove_repo,
            hub_unlink_repo,
            hub_scan_directory,
            hub_set_favorite,
            hub_set_tags,
            hub_set_display_name,
            hub_set_project_intro,
            hub_sync_repo_meta_from_disk,
            repo_resolve_worktree_path,
            hub_touch_repo,
            hub_refresh_heads,
            repo_ls_tree,
            repo_warm_tree_cache,
            repo_log,
            repo_list_refs,
            repo_latest_commit,
            repo_rev_count,
            repo_path_last_commit,
            repo_paths_last_commit,
            repo_remotes,
            repo_show_commit,
            repo_blob_text,
            repo_blob_base64,
            repo_status,
            repo_stage,
            repo_commit,
            repo_list_releases,
            repo_save_releases,
            repo_import_release_asset,
            repo_import_release_sources,
            repo_delete_release_asset,
            repo_resolve_release_asset_path,
            import_zip,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
