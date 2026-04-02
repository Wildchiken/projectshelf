mod db;
mod git;

use base64::Engine;
use db::{Database, RepoRecord};
use tauri::Manager;
use git::{
    clone_repo, discover_repos_under, head_sha, latest_commit_at, list_refs, list_remotes,
    log_oneline_for_rev, last_commit_for_path, resolve_git_binary, resolve_repo, rev_list_count, show_blob,
    show_commit_patch, CommitSummary, GitError, RefLists, RemoteInfo, StatusLine, TreeEntry,
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
        .map(|p| p.data_local_dir().join("deskvio"))
        .ok_or_else(|| "could not resolve application data directory".to_string())
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

fn legacy_repo_root() -> Result<PathBuf, String> {
    let root = app_data_root()?.join("repositories");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

fn visible_repo_root_candidate() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join("repositories"))
}

fn ensure_writable_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.permissions().readonly() {
        return Err("directory is read-only".to_string());
    }
    Ok(())
}

fn default_repo_root() -> Result<PathBuf, String> {
    if let Some(visible_root) = visible_repo_root_candidate() {
        if ensure_writable_dir(&visible_root).is_ok() {
            return Ok(visible_root);
        }
    }
    let fallback = legacy_repo_root()?;
    ensure_writable_dir(&fallback)?;
    Ok(fallback)
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

fn is_allowed_delete_path(path: &Path) -> bool {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(r) = default_repo_root() {
        roots.push(r);
    }
    if let Ok(r) = legacy_repo_root() {
        roots.push(r);
    }
    roots.into_iter().any(|root| path.starts_with(root))
}

pub struct AppState {
    db: Mutex<Database>,
    git_bin: PathBuf,
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
        let rec = db
            .insert_repo(
                &path.to_string_lossy(),
                name,
                ctx.bare,
                head,
            )
            .map_err(map_db_err)?;
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
    db.insert_repo(
        &canon.to_string_lossy(),
        name,
        ctx.bare,
        head.clone(),
    )
    .map_err(map_db_err)
}

#[tauri::command]
fn hub_default_repo_root() -> Result<String, String> {
    let root = default_repo_root()?;
    Ok(root.to_string_lossy().to_string())
}

#[tauri::command]
fn hub_legacy_repo_root() -> Result<String, String> {
    let root = legacy_repo_root()?;
    Ok(root.to_string_lossy().to_string())
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
    let base = if let Some(dest) = dest_parent {
        let p = PathBuf::from(dest.trim());
        if p.as_os_str().is_empty() {
            default_repo_root()?
        } else {
            p
        }
    } else {
        default_repo_root()?
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
    db.insert_repo(
        &canon.to_string_lossy(),
        display_name,
        ctx.bare,
        head,
    )
    .map_err(map_db_err)
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
    if !is_allowed_delete_path(&canon) {
        return Err("refusing to delete repository outside allowed roots".into());
    }
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
    db.set_tags(id, &tags).map_err(map_db_err)
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
    db.set_project_intro(id, intro).map_err(map_db_err)
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
    let file_name = src_can
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "asset.bin".to_string());
    let (asset_id, rel_path, dst) = make_unique_asset_target(&repo_root, &release_id, &file_name);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(&src_can, &dst).map_err(|e| e.to_string())?;
    let meta = fs::metadata(&dst).map_err(|e| e.to_string())?;
    Ok(ReleaseAsset {
        id: asset_id,
        name: file_name,
        stored_path: rel_path,
        original_path: src_can.to_string_lossy().to_string(),
        size_bytes: meta.len(),
        added_at: now_unix(),
    })
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
fn repo_ls_tree(
    state: tauri::State<'_, AppState>,
    id: i64,
    rev: Option<String>,
) -> Result<Vec<TreeEntry>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let r = db
        .get(id)
        .map_err(map_db_err)?
        .ok_or_else(|| "repository not found".to_string())?;
    drop(db);
    let ctx = load_ctx(&state.git_bin, &r.path)?;
    let treeish = rev.unwrap_or_else(|| "HEAD".into());
    git::ls_tree(&state.git_bin, &ctx, &treeish).map_err(map_git_err)
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
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let p = path.trim().to_string();
        if p.is_empty() {
            continue;
        }
        let commit = last_commit_for_path(&state.git_bin, &ctx, &rev, &p).map_err(map_git_err)?;
        out.push(RepoPathCommit { path: p, commit });
    }
    Ok(out)
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

    let base_dest = if let Some(p) = dest_parent {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            default_repo_root()?
        } else {
            PathBuf::from(trimmed)
        }
    } else {
        let dir = default_repo_root()?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        dir.join(format!("import-{}", ts))
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

    #[test]
    fn zip_depth_counts_components() {
        assert_eq!(zip_path_depth(Path::new("a/b/c")), 3);
        assert_eq!(zip_path_depth(Path::new("readme.md")), 1);
    }

    #[test]
    fn allowed_delete_path_rejects_current_workspace() {
        let cwd = std::env::current_dir().expect("cwd");
        let candidate = cwd.join("tmp-delete-check");
        assert!(!is_allowed_delete_path(&candidate));
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
            std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
            migrate_legacy_app_data(&app_dir)?;
            let db_path = app_dir.join("hub.db");
            let db = Database::open(&db_path).map_err(|e| e.to_string())?;
            let git_bin = resolve_git_binary();
            app.manage(AppState {
                db: Mutex::new(db),
                git_bin,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            hub_list_repos,
            hub_search,
            hub_add_repo,
            hub_default_repo_root,
            hub_legacy_repo_root,
            hub_clone_repo,
            hub_remove_repo,
            hub_unlink_repo,
            hub_scan_directory,
            hub_set_favorite,
            hub_set_tags,
            hub_set_display_name,
            hub_set_project_intro,
            hub_touch_repo,
            hub_refresh_heads,
            repo_ls_tree,
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
            repo_delete_release_asset,
            import_zip,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
