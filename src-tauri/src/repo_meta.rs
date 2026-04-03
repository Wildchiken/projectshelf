use crate::db::Database;
use rusqlite::Error as SqliteError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoProjectFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_version() -> u32 {
    1
}

pub fn project_json_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".deskvio").join("project.json")
}

pub fn read_project_file(repo_root: &Path) -> Option<RepoProjectFile> {
    let p = project_json_path(repo_root);
    let s = fs::read_to_string(&p).ok()?;
    serde_json::from_str::<RepoProjectFile>(&s).ok()
}

pub fn write_project_file(
    repo_root: &Path,
    intro: Option<&str>,
    tags: &[String],
) -> Result<(), String> {
    let dir = repo_root.join(".deskvio");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let data = RepoProjectFile {
        version: 1,
        intro: intro.map(str::to_string),
        tags: tags.to_vec(),
    };
    let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    let path = project_json_path(repo_root);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json.as_bytes()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn merge_disk_into_db(db: &Database, id: i64, repo_root: &Path) -> Result<bool, SqliteError> {
    let Some(file) = read_project_file(repo_root) else {
        return Ok(false);
    };
    db.set_project_intro(id, file.intro.clone())?;
    db.set_tags(id, &file.tags)?;
    Ok(true)
}

pub fn persist_from_db(db: &Database, id: i64) -> Result<(), String> {
    let rec = db
        .get(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "repository not found".to_string())?;
    let root = Path::new(&rec.path);
    write_project_file(
        root,
        rec.project_intro.as_deref(),
        &rec.tags,
    )
}

pub fn resolve_worktree_file_path(repo_path: &str, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("path is empty".to_string());
    }
    let rel_norm = rel.replace('\\', "/");
    if rel_norm.starts_with('/') {
        return Err("absolute paths are not allowed".to_string());
    }
    for seg in rel_norm.split('/') {
        if seg.is_empty() {
            continue;
        }
        if seg == ".." {
            return Err("invalid path segment".to_string());
        }
    }
    let root = Path::new(repo_path)
        .canonicalize()
        .map_err(|e| format!("repository path: {}", e))?;
    let mut joined = root.clone();
    for seg in rel_norm.split('/').filter(|s| !s.is_empty()) {
        joined.push(seg);
    }
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("resolved path does not exist: {}", e))?;
    if !canon.starts_with(&root) {
        return Err("path escapes repository root".to_string());
    }
    if !canon.is_file() {
        return Err("not a regular file".to_string());
    }
    Ok(canon)
}
