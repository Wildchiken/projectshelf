use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("git executable not found or failed: {0}")]
    Run(String),
    #[error("invalid utf-8 in git output")]
    Utf8,
}

#[derive(Debug, Clone)]
pub struct RepoContext {
    pub root: PathBuf,
    pub bare: bool,
}

pub fn resolve_git_binary() -> PathBuf {
    if let Ok(p) = std::env::var("PORTABLE_GIT_PATH") {
        let pb = PathBuf::from(p);
        if pb.as_os_str().is_empty() {
            return PathBuf::from("git");
        }
        return pb;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            #[cfg(target_os = "windows")]
            {
                let win = dir.join("portable-git").join("cmd").join("git.exe");
                if win.exists() {
                    return win;
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let mac = dir.join("portable-git").join("bin").join("git");
                if mac.exists() {
                    return mac;
                }
            }
        }
    }
    PathBuf::from("git")
}

fn run_git(git: &Path, args: &[String]) -> Result<String, GitError> {
    let out = Command::new(git)
        .args(args)
        .output()
        .map_err(|e| GitError::Run(e.to_string()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("git exited with {}", out.status)
        } else {
            stderr
        };
        return Err(GitError::Run(msg));
    }
    String::from_utf8(out.stdout).map_err(|_| GitError::Utf8)
}

pub fn resolve_repo(git: &Path, path: &Path) -> Result<RepoContext, GitError> {
    let path_str = path.to_string_lossy();
    let is_bare_out = run_git(
        git,
        &[
            "-C".into(),
            path_str.to_string(),
            "rev-parse".into(),
            "--is-bare-repository".into(),
        ],
    )?;
    let bare = is_bare_out.trim() == "true";
    Ok(RepoContext {
        root: path.to_path_buf(),
        bare,
    })
}

pub fn head_sha(git: &Path, ctx: &RepoContext) -> Result<String, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "rev-parse".into(),
            "HEAD".into(),
        ],
    )?;
    Ok(out.trim().to_string())
}

pub fn rev_parse_verify(git: &Path, ctx: &RepoContext, spec: &str) -> Result<String, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "rev-parse".into(),
            "-q".into(),
            "--verify".into(),
            spec.to_string(),
        ],
    )?;
    let s = out.trim().to_string();
    if s.is_empty() {
        return Err(GitError::Run("rev-parse returned empty".into()));
    }
    Ok(s)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    pub path: String,
    pub mode: String,
    pub object_type: String,
    pub object_id: String,
}

fn parse_ls_tree_lines(out: &str) -> Vec<TreeEntry> {
    let mut entries = Vec::new();
    for line in out.lines() {
        let Some((meta, path)) = line.split_once('\t') else {
            continue;
        };
        let parts: Vec<&str> = meta.split_whitespace().collect();
        if parts.len() >= 3 {
            entries.push(TreeEntry {
                path: path.to_string(),
                mode: parts[0].to_string(),
                object_type: parts[1].to_string(),
                object_id: parts[2].to_string(),
            });
        }
    }
    entries
}

pub fn ls_tree(git: &Path, ctx: &RepoContext, treeish: &str) -> Result<Vec<TreeEntry>, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "ls-tree".into(),
            "-r".into(),
            treeish.to_string(),
        ],
    )?;
    Ok(parse_ls_tree_lines(&out))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitSummary {
    pub id: String,
    pub subject: String,
    pub author: String,
    pub date_unix: i64,
}

fn parse_commit_line(line: &str) -> Option<CommitSummary> {
    let mut parts = line.splitn(4, '\t');
    let id = parts.next()?.to_string();
    if id.is_empty() {
        return None;
    }
    let subject = parts.next().unwrap_or("").to_string();
    let author = parts.next().unwrap_or("").to_string();
    let date = parts.next().unwrap_or("0").parse().unwrap_or(0i64);
    Some(CommitSummary {
        id,
        subject,
        author,
        date_unix: date,
    })
}

pub fn log_oneline_for_rev(
    git: &Path,
    ctx: &RepoContext,
    rev: &str,
    limit: usize,
) -> Result<Vec<CommitSummary>, GitError> {
    let c = ctx.root.to_string_lossy();
    let lim = limit.to_string();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "log".into(),
            "-n".into(),
            lim,
            "--format=%H%x09%s%x09%an%x09%ct".into(),
            rev.to_string(),
        ],
    )?;
    let mut list = Vec::new();
    for line in out.lines() {
        if let Some(c) = parse_commit_line(line) {
            list.push(c);
        }
    }
    Ok(list)
}

pub fn latest_commit_at(git: &Path, ctx: &RepoContext, rev: &str) -> Result<Option<CommitSummary>, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "log".into(),
            "-1".into(),
            "--format=%H%x09%s%x09%an%x09%ct".into(),
            rev.to_string(),
        ],
    )?;
    Ok(out.lines().next().and_then(parse_commit_line))
}

pub fn rev_list_count(git: &Path, ctx: &RepoContext, rev: &str) -> Result<usize, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "rev-list".into(),
            "--count".into(),
            rev.to_string(),
        ],
    )?;
    let n = out.trim().parse().unwrap_or(0);
    Ok(n)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefLists {
    pub branches: Vec<String>,
    pub tags: Vec<String>,
}

pub fn list_refs(git: &Path, ctx: &RepoContext, max_tags: usize) -> Result<RefLists, GitError> {
    let c = ctx.root.to_string_lossy();
    let branches_out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "branch".into(),
            "-a".into(),
            "--format=%(refname:short)".into(),
        ],
    )?;
    let mut seen = std::collections::HashSet::new();
    let mut branches = Vec::new();
    for line in branches_out.lines() {
        let s = line.trim().to_string();
        if s.is_empty() || s == "HEAD" {
            continue;
        }
        if seen.insert(s.clone()) {
            branches.push(s);
        }
    }

    let tags_out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "tag".into(),
            "-l".into(),
            "--sort=-creatordate".into(),
        ],
    )?;
    let tags: Vec<String> = tags_out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
        .take(max_tags)
        .collect();

    Ok(RefLists { branches, tags })
}

pub fn last_commit_for_path(
    git: &Path,
    ctx: &RepoContext,
    rev: &str,
    path: &str,
) -> Result<Option<CommitSummary>, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "log".into(),
            "-1".into(),
            "--format=%H%x09%s%x09%an%x09%ct".into(),
            rev.to_string(),
            "--".into(),
            path.to_string(),
        ],
    )?;
    Ok(out.lines().next().and_then(parse_commit_line))
}

fn path_spec_touched_by_log_line(path_spec: &str, changed_path: &str) -> bool {
    let changed_path = changed_path.trim();
    if changed_path.is_empty() {
        return false;
    }
    if path_spec.ends_with('/') {
        let base = path_spec.trim_end_matches('/');
        if base.is_empty() {
            return false;
        }
        changed_path == base || changed_path.starts_with(&format!("{}/", base))
    } else {
        changed_path == path_spec
    }
}

const LAST_COMMIT_BATCH_CHUNK: usize = 100;
const LAST_COMMIT_BATCH_LOG_MAX: u32 = 100_000;

pub fn last_commits_for_paths(
    git: &Path,
    ctx: &RepoContext,
    rev: &str,
    paths: &[String],
) -> Result<Vec<(String, Option<CommitSummary>)>, GitError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let c = ctx.root.to_string_lossy();
    let mut combined = Vec::with_capacity(paths.len());

    for chunk in paths.chunks(LAST_COMMIT_BATCH_CHUNK) {
        let mut found: std::collections::HashMap<String, CommitSummary> =
            std::collections::HashMap::new();

        let mut args: Vec<String> = vec![
            "-C".into(),
            c.to_string(),
            "log".into(),
            "-n".into(),
            LAST_COMMIT_BATCH_LOG_MAX.to_string(),
            "--format=%H%x09%s%x09%an%x09%ct".into(),
            "--name-only".into(),
            rev.to_string(),
            "--".into(),
        ];
        for p in chunk {
            args.push(p.clone());
        }

        let stdout = run_git(git, &args)?;
        let mut current: Option<CommitSummary> = None;

        for line in stdout.lines() {
            if found.len() == chunk.len() {
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            if let Some(cs) = parse_commit_line(line) {
                current = Some(cs);
                continue;
            }
            let changed = line.trim();
            if changed.is_empty() {
                continue;
            }
            let Some(ref cur) = current else {
                continue;
            };
            let to_add: Vec<String> = chunk
                .iter()
                .filter(|p| !found.contains_key(*p))
                .filter(|p| path_spec_touched_by_log_line(p, changed))
                .cloned()
                .collect();
            for p in to_add {
                found.insert(p, cur.clone());
            }
        }

        for p in chunk {
            let commit = if let Some(cs) = found.get(p) {
                Some(cs.clone())
            } else {
                last_commit_for_path(git, ctx, rev, p)?
            };
            combined.push((p.clone(), commit));
        }
    }

    Ok(combined)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub name: String,
    pub fetch_url: String,
}

pub fn list_remotes(git: &Path, ctx: &RepoContext) -> Result<Vec<RemoteInfo>, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &["-C".into(), c.to_string(), "remote".into(), "-v".into()],
    )?;
    let mut by_name: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[2] == "(fetch)" {
            by_name.insert(parts[0].to_string(), parts[1].to_string());
        }
    }
    Ok(by_name
        .into_iter()
        .map(|(name, fetch_url)| RemoteInfo { name, fetch_url })
        .collect())
}

pub fn show_commit_patch(git: &Path, ctx: &RepoContext, commit: &str) -> Result<String, GitError> {
    let c = ctx.root.to_string_lossy();
    run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "show".into(),
            "--no-color".into(),
            "--format=fuller".into(),
            commit.to_string(),
        ],
    )
}

pub fn show_blob(git: &Path, ctx: &RepoContext, spec: &str) -> Result<Vec<u8>, GitError> {
    let c = ctx.root.to_string_lossy();
    let out = Command::new(git)
        .args(["-C", c.as_ref(), "show", spec])
        .output()
        .map_err(|e| GitError::Run(e.to_string()))?;
    if !out.status.success() {
        return Err(GitError::Run(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(out.stdout)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusLine {
    pub x: String,
    pub y: String,
    pub path: String,
}

pub fn status_porcelain(git: &Path, ctx: &RepoContext) -> Result<Vec<StatusLine>, GitError> {
    if ctx.bare {
        return Ok(Vec::new());
    }
    let c = ctx.root.to_string_lossy();
    let out = run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "status".into(),
            "--porcelain=v1".into(),
            "-z".into(),
        ],
    )?;
    let mut v = Vec::new();
    for chunk in out.split('\0') {
        if chunk.len() < 4 {
            continue;
        }
        let xs = chunk.chars().next().unwrap_or(' ').to_string();
        let ys = chunk.chars().nth(1).unwrap_or(' ').to_string();
        let rest = &chunk[3..];
        let path = if let Some((a, _b)) = rest.split_once(" -> ") {
            a.to_string()
        } else {
            rest.to_string()
        };
        v.push(StatusLine {
            x: xs,
            y: ys,
            path,
        });
    }
    Ok(v)
}

pub fn add_paths(git: &Path, ctx: &RepoContext, paths: &[String]) -> Result<(), GitError> {
    if ctx.bare {
        return Err(GitError::Run("cannot stage in bare repository".into()));
    }
    let c = ctx.root.to_string_lossy();
    let mut args: Vec<String> = vec!["-C".into(), c.into(), "add".into(), "--".into()];
    for p in paths {
        args.push(p.clone());
    }
    run_git(git, &args)?;
    Ok(())
}

pub fn commit_message(git: &Path, ctx: &RepoContext, message: &str) -> Result<String, GitError> {
    if ctx.bare {
        return Err(GitError::Run("cannot commit in bare repository".into()));
    }
    let c = ctx.root.to_string_lossy();
    run_git(
        git,
        &[
            "-C".into(),
            c.to_string(),
            "commit".into(),
            "-m".into(),
            message.to_string(),
        ],
    )
}

pub fn clone_repo(git: &Path, url: &str, dest: &Path) -> Result<(), GitError> {
    let out = Command::new(git)
        .args([
            "clone",
            "--",
            url,
            dest.to_string_lossy().as_ref(),
        ])
        .output()
        .map_err(|e| GitError::Run(e.to_string()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("git clone exited with {}", out.status)
        } else {
            stderr
        };
        return Err(GitError::Run(msg));
    }
    Ok(())
}

pub fn discover_repos_under(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    use std::collections::HashSet;
    use walkdir::WalkDir;

    let mut found: HashSet<PathBuf> = HashSet::new();
    for entry in WalkDir::new(root).max_depth(max_depth).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.file_name() == Some(std::ffi::OsStr::new(".git")) {
            if let Some(parent) = p.parent() {
                if let Ok(can) = parent.canonicalize() {
                    found.insert(can);
                }
            }
        }
    }
    found.into_iter().collect()
}
