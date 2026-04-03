import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type RepoRecord = {
  id: number;
  path: string;
  displayName: string | null;
  projectIntro: string | null;
  isFavorite: boolean;
  tags: string[];
  lastOpenedAt: number | null;
  isBare: boolean;
  lastHead: string | null;
  createdAt: number;
};

export type TreeEntry = {
  path: string;
  mode: string;
  objectType: string;
  objectId: string;
};

export type CommitSummary = {
  id: string;
  subject: string;
  author: string;
  dateUnix: number;
};

export type StatusLine = {
  x: string;
  y: string;
  path: string;
};

export type ReleaseAsset = {
  id: string;
  name: string;
  storedPath: string;
  originalPath: string;
  sizeBytes: number;
  addedAt: number;
};

export type ReleaseEntry = {
  id: string;
  version: string;
  title: string;
  notes: string;
  sourceUrl: string;
  assets: ReleaseAsset[];
  createdAt: number;
  updatedAt: number;
};

export async function hubListRepos(): Promise<RepoRecord[]> {
  return invoke("hub_list_repos");
}

export type DbStatusInfo = {
  status: "ok" | "repaired" | "temp";
  dbPath: string;
};

export async function appDbStatus(): Promise<DbStatusInfo> {
  return invoke("app_db_status");
}

export async function hubPruneMissingRepos(): Promise<number> {
  return invoke("hub_prune_missing_repos");
}

export async function hubSearch(query: string): Promise<RepoRecord[]> {
  return invoke("hub_search", { query });
}

export async function hubAddRepo(path: string): Promise<RepoRecord> {
  return invoke("hub_add_repo", { path });
}

export async function hubDefaultRepoRoot(): Promise<string> {
  return invoke("hub_default_repo_root");
}

export async function hubLegacyRepoRoot(): Promise<string> {
  return invoke("hub_legacy_repo_root");
}

export async function hubCloneRepo(
  url: string,
  destParent?: string | null,
): Promise<RepoRecord> {
  return invoke("hub_clone_repo", { url, destParent: destParent ?? null });
}

export type CloneProgressPayload = { sessionId: string; line: string };
export type CloneDonePayload = { sessionId: string; ok: boolean; error: string | null };

export async function hubCloneRepoStream(
  url: string,
  destParent?: string | null,
): Promise<string> {
  return invoke("hub_clone_repo_stream", { url, destParent: destParent ?? null });
}

export type CancelCloneResult = {
  sessionId: string;
  target: string;
  killed: boolean;
  removed: boolean;
  stillExists: boolean;
  error: string | null;
};

export async function hubCancelClone(sessionId: string): Promise<CancelCloneResult> {
  return invoke("hub_cancel_clone", { sessionId });
}

export async function hubCheckCloneConflict(url: string): Promise<RepoRecord | null> {
  return invoke("hub_check_clone_conflict", { url });
}

export type FetchResetResult = {
  ok: boolean;
  dirty: boolean;
  stashed: boolean;
  error: string | null;
};

export async function hubOverwriteFetchReset(id: number): Promise<FetchResetResult> {
  return invoke("hub_overwrite_fetch_reset", { id });
}

export async function hubOverwriteFetchResetAuto(
  id: number,
): Promise<FetchResetResult> {
  return invoke("hub_overwrite_fetch_reset_auto", { id });
}

export function onCloneProgress(
  cb: (payload: CloneProgressPayload) => void,
): Promise<UnlistenFn> {
  return listen<CloneProgressPayload>("clone-progress", (e) => cb(e.payload));
}

export function onCloneDone(
  cb: (payload: CloneDonePayload) => void,
): Promise<UnlistenFn> {
  return listen<CloneDonePayload>("clone-done", (e) => cb(e.payload));
}

export async function hubRemoveRepo(id: number): Promise<void> {
  return invoke("hub_remove_repo", { id });
}

export async function hubUnlinkRepo(id: number): Promise<void> {
  return invoke("hub_unlink_repo", { id });
}

export async function hubScanDirectory(
  root: string,
  maxDepth?: number,
): Promise<RepoRecord[]> {
  return invoke("hub_scan_directory", { root, maxDepth });
}

export async function hubSetFavorite(
  id: number,
  favorite: boolean,
): Promise<void> {
  return invoke("hub_set_favorite", { id, favorite });
}

export async function hubSetTags(id: number, tags: string[]): Promise<void> {
  return invoke("hub_set_tags", { id, tags });
}

export async function hubSetDisplayName(
  id: number,
  name: string | null,
): Promise<void> {
  return invoke("hub_set_display_name", { id, name });
}

export async function hubSetProjectIntro(
  id: number,
  intro: string | null,
): Promise<void> {
  return invoke("hub_set_project_intro", { id, intro });
}

export async function hubSyncRepoMetaFromDisk(id: number): Promise<RepoRecord> {
  return invoke("hub_sync_repo_meta_from_disk", { id });
}

export async function repoResolveWorktreePath(
  id: number,
  relativePath: string,
): Promise<string> {
  return invoke("repo_resolve_worktree_path", { id, relativePath });
}

export async function hubTouchRepo(id: number): Promise<void> {
  return invoke("hub_touch_repo", { id });
}

export type HubRefreshHeadsSummary = {
  total: number;
  ok: number;
  failed: number;
};

export async function hubRefreshHeads(): Promise<HubRefreshHeadsSummary> {
  return invoke("hub_refresh_heads");
}

export async function repoLsTree(
  id: number,
  rev?: string,
): Promise<TreeEntry[]> {
  return invoke("repo_ls_tree", { id, rev });
}

export async function repoWarmTreeCache(
  id: number,
  rev?: string,
): Promise<void> {
  return invoke("repo_warm_tree_cache", { id, rev });
}

export type RefLists = {
  branches: string[];
  tags: string[];
};

export type RemoteInfo = {
  name: string;
  fetchUrl: string;
};

export type RepoPathCommit = {
  path: string;
  commit: CommitSummary | null;
};

export async function repoLog(
  id: number,
  limit?: number,
  rev?: string | null,
): Promise<CommitSummary[]> {
  return invoke("repo_log", { id, limit, rev: rev ?? null });
}

export async function repoListRefs(id: number): Promise<RefLists> {
  return invoke("repo_list_refs", { id });
}

export async function repoLatestCommit(
  id: number,
  rev: string,
): Promise<CommitSummary | null> {
  return invoke("repo_latest_commit", { id, rev });
}

export async function repoRevCount(id: number, rev: string): Promise<number> {
  return invoke("repo_rev_count", { id, rev });
}

export async function repoPathLastCommit(
  id: number,
  rev: string,
  path: string,
): Promise<CommitSummary | null> {
  return invoke("repo_path_last_commit", { id, rev, path });
}

export async function repoPathsLastCommit(
  id: number,
  rev: string,
  paths: string[],
): Promise<RepoPathCommit[]> {
  return invoke("repo_paths_last_commit", { id, rev, paths });
}

export async function repoRemotes(id: number): Promise<RemoteInfo[]> {
  return invoke("repo_remotes", { id });
}

export async function repoShowCommit(
  id: number,
  commit: string,
): Promise<string> {
  return invoke("repo_show_commit", { id, commit });
}

export async function repoBlobText(
  id: number,
  spec: string,
): Promise<string> {
  return invoke("repo_blob_text", { id, spec });
}

export async function repoBlobBase64(
  id: number,
  spec: string,
): Promise<string> {
  return invoke("repo_blob_base64", { id, spec });
}

export async function repoStatus(id: number): Promise<StatusLine[]> {
  return invoke("repo_status", { id });
}

export async function repoStage(id: number, paths: string[]): Promise<void> {
  return invoke("repo_stage", { id, paths });
}

export async function repoCommit(
  id: number,
  message: string,
): Promise<string> {
  return invoke("repo_commit", { id, message });
}

export async function repoListReleases(id: number): Promise<ReleaseEntry[]> {
  return invoke("repo_list_releases", { id });
}

export async function repoSaveReleases(
  id: number,
  releases: ReleaseEntry[],
): Promise<ReleaseEntry[]> {
  return invoke("repo_save_releases", { id, releases });
}

export async function repoImportReleaseAsset(
  id: number,
  releaseId: string,
  sourcePath: string,
): Promise<ReleaseAsset> {
  return invoke("repo_import_release_asset", {
    id,
    releaseId,
    sourcePath,
  });
}

export async function repoImportReleaseSources(
  id: number,
  releaseId: string,
  sourcePaths: string[],
): Promise<ReleaseAsset[]> {
  return invoke("repo_import_release_sources", {
    id,
    releaseId,
    sourcePaths,
  });
}

export async function repoDeleteReleaseAsset(
  id: number,
  storedPath: string,
): Promise<void> {
  return invoke("repo_delete_release_asset", {
    id,
    storedPath,
  });
}

export async function repoResolveReleaseAssetPath(
  id: number,
  storedPath: string,
): Promise<string> {
  return invoke("repo_resolve_release_asset_path", {
    id,
    storedPath,
  });
}

export async function importZip(
  zipPath: string,
  destParent?: string | null,
): Promise<RepoRecord[]> {
  return invoke("import_zip", {
    zipPath,
    destParent: destParent ?? null,
  });
}
