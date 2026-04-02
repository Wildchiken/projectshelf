import { open } from "@tauri-apps/plugin-dialog";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import type { RepoRecord } from "./api";
import {
  hubAddRepo,
  hubCancelClone,
  hubCloneRepoStream,
  hubDefaultRepoRoot,
  hubListRepos,
  hubScanDirectory,
  hubSearch,
  hubSetFavorite,
  hubTouchRepo,
  hubRefreshHeads,
  importZip,
  repoWarmTreeCache,
  onCloneDone,
  onCloneProgress,
} from "./api";

type Props = {
  onOpenRepo: (r: RepoRecord) => void;
  locale: "zh-CN" | "en-US";
  layoutMode: "comfortable" | "compact";
  repoRoot: string;
  refreshToken?: number;
};

type SortMode = "favorite_first" | "recent_first" | "name_asc" | "created_desc";

export function HubView({
  onOpenRepo,
  locale,
  layoutMode,
  repoRoot,
  refreshToken,
}: Props) {
  const APP_REPO_ROOT_KEY = "deskvio-repo-root";
  function getEffectiveRepoRoot() {
    const stored =
      typeof window !== "undefined" ? localStorage.getItem(APP_REPO_ROOT_KEY) ?? "" : "";
    // Priority: in-memory setting -> persisted setting -> empty (falls back to backend default).
    return repoRoot.trim() || stored.trim();
  }

  const [repos, setRepos] = useState<RepoRecord[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);
  const [notice, setNotice] = useState<string | null>(null);
  const [moreOpen, setMoreOpen] = useState(false);
  const [sortMode, setSortMode] = useState<SortMode>("created_desc");
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [cloneUrl, setCloneUrl] = useState("");
  const [cloneSessionId, setCloneSessionId] = useState<string | null>(null);
  const [cloneLog, setCloneLog] = useState<string[]>([]);
  const [cloneResult, setCloneResult] = useState<{ ok: boolean; error?: string } | null>(null);
  const [resolvedDefaultRoot, setResolvedDefaultRoot] = useState("");
  const cloneLogRef = useRef<HTMLPreElement>(null);
  const moreRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  // Avoid repeatedly scanning the same root during incidental re-renders/refreshes.
  const lastScannedRepoRootRef = useRef<string>(getEffectiveRepoRoot());
  const [effectiveColumns, setEffectiveColumns] = useState<number>(1);
  const isZh = locale === "zh-CN";
  const sortOptions = isZh
    ? [
        { value: "favorite_first" as const, label: "收藏优先" },
        { value: "recent_first" as const, label: "最近打开优先" },
        { value: "name_asc" as const, label: "名称 A→Z" },
        { value: "created_desc" as const, label: "添加时间（新→旧）" },
      ]
    : [
        { value: "favorite_first" as const, label: "Favorites First" },
        { value: "recent_first" as const, label: "Recently Opened" },
        { value: "name_asc" as const, label: "Name A→Z" },
        { value: "created_desc" as const, label: "Recently Added" },
      ];
  const ui = isZh
    ? {
        moreActions: "更多操作",
        checkedTotal: (n: number) => `已检查 ${n} 个仓库`,
        headUpdated: (n: number) => `HEAD 已更新 ${n} 个`,
        headFailed: (n: number) => `${n} 个无法解析（路径失效或无提交）`,
        unstar: "取消收藏",
        star: "收藏",
        noCommit: "无提交",
        tagsCount: (n: number) => `标签 ${n}`,
        noTags: "无标签",
      }
    : {
        moreActions: "More actions",
        checkedTotal: (n: number) => `Checked ${n} repositories`,
        headUpdated: (n: number) => `Updated HEAD for ${n}`,
        headFailed: (n: number) => `${n} unresolved (missing path or no commits)`,
        unstar: "Unfavorite",
        star: "Favorite",
        noCommit: "No commits",
        tagsCount: (n: number) => `${n} tags`,
        noTags: "No tags",
      };

  const displayedRepos = useMemo(() => {
    const filtered = favoritesOnly ? repos.filter((r) => r.isFavorite) : repos;
    const withIndex = filtered.map((repo, index) => ({ repo, index }));
    withIndex.sort((a, b) => {
      const ar = a.repo;
      const br = b.repo;
      if (sortMode === "favorite_first") {
        if (ar.isFavorite !== br.isFavorite) return ar.isFavorite ? -1 : 1;
        const ao = ar.lastOpenedAt ?? 0;
        const bo = br.lastOpenedAt ?? 0;
        if (ao !== bo) return bo - ao;
      } else if (sortMode === "recent_first") {
        const ao = ar.lastOpenedAt ?? 0;
        const bo = br.lastOpenedAt ?? 0;
        if (ao !== bo) return bo - ao;
      } else if (sortMode === "name_asc") {
        const an = (ar.displayName ?? ar.path.split(/[/\\]/).filter(Boolean).pop() ?? ar.path).toLowerCase();
        const bn = (br.displayName ?? br.path.split(/[/\\]/).filter(Boolean).pop() ?? br.path).toLowerCase();
        const cmp = an.localeCompare(bn);
        if (cmp !== 0) return cmp;
      } else if (sortMode === "created_desc") {
        if (ar.createdAt !== br.createdAt) return br.createdAt - ar.createdAt;
      }
      return a.index - b.index;
    });
    return withIndex.map((item) => item.repo);
  }, [repos, favoritesOnly, sortMode]);

  useEffect(() => {
    void hubDefaultRepoRoot().then(setResolvedDefaultRoot).catch(() => {});
  }, []);

  useEffect(() => {
    if (!moreOpen) return;
    const close = (e: MouseEvent) => {
      if (moreRef.current && !moreRef.current.contains(e.target as Node)) {
        setMoreOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [moreOpen]);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      if (query.trim() === "") {
        const nextRoot = getEffectiveRepoRoot().trim();
        if (nextRoot.length > 0 && nextRoot !== lastScannedRepoRootRef.current && !busyRef.current) {
          setBusy(true);
          try {
            await hubScanDirectory(nextRoot, 12);
            lastScannedRepoRootRef.current = nextRoot;
          } finally {
            setBusy(false);
          }
        }
        const list = await hubListRepos();
        setRepos(list);
        return;
      }

      const list = await hubSearch(query.trim());
      setRepos(list);
    } catch (e) {
      setError(String(e));
    }
  }, [query, repoRoot]);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshToken]);

  useEffect(() => {
    if (query.trim() !== "") return;
    const candidates = [...repos]
      .filter((r) => r.lastOpenedAt != null || r.isFavorite)
      .sort((a, b) => {
        if (a.isFavorite !== b.isFavorite) return a.isFavorite ? -1 : 1;
        return (b.lastOpenedAt ?? 0) - (a.lastOpenedAt ?? 0);
      })
      .slice(0, 4)
      .map((r) => r.id);
    if (candidates.length === 0) return;
    let cancelled = false;
    let idleId: number | null = null;
    let timeoutId: ReturnType<typeof setTimeout> | null = null;
    const run = () => {
      if (cancelled) return;
      for (const rid of candidates) {
        void repoWarmTreeCache(rid, "HEAD").catch(() => {});
      }
    };
    if (typeof window.requestIdleCallback === "function") {
      idleId = window.requestIdleCallback(run);
    } else {
      timeoutId = window.setTimeout(run, 400);
    }
    return () => {
      cancelled = true;
      if (idleId != null && typeof window.cancelIdleCallback === "function") {
        window.cancelIdleCallback(idleId);
      }
      if (timeoutId != null) window.clearTimeout(timeoutId);
    };
  }, [repos, query, refreshToken]);

  useEffect(() => {
    if (!cloneSessionId) return;
    const unlistens: Promise<() => void>[] = [];
    unlistens.push(
      onCloneProgress((p) => {
        if (p.sessionId !== cloneSessionId) return;
        setCloneLog((prev) => {
          const next = [...prev, p.line];
          return next.length > 200 ? next.slice(-200) : next;
        });
        requestAnimationFrame(() => {
          cloneLogRef.current?.scrollTo(0, cloneLogRef.current.scrollHeight);
        });
      }),
    );
    unlistens.push(
      onCloneDone((p) => {
        if (p.sessionId !== cloneSessionId) return;
        setCloneResult({ ok: p.ok, error: p.error ?? undefined });
        setBusy(false);
        if (p.ok) {
          void refresh();
          setTimeout(() => {
            setCloneOpen(false);
            setCloneUrl("");
            setCloneLog([]);
            setCloneResult(null);
            setCloneSessionId(null);
          }, 1500);
        }
      }),
    );
    return () => {
      for (const u of unlistens) void u.then((fn) => fn());
    };
  }, [cloneSessionId, refresh]);

  useEffect(() => {
    const listEl = listRef.current;
    if (!listEl) return;

    const compute = () => {
      const width = listEl.clientWidth;
      if (width <= 0) return;
      const styles = getComputedStyle(listEl);
      const gap = Number.parseFloat(styles.columnGap || styles.gap || "14") || 14;
      let next = 1;
      const minCardWidth = layoutMode === "compact" ? 260 : 320;
      const maxByDensity = layoutMode === "compact" ? 5 : 3;
      next = Math.min(maxByDensity, Math.max(1, Math.floor((width + gap) / (minCardWidth + gap))));
      while (next > 1) {
        const cardWidth = (width - gap * (next - 1)) / next;
        if (cardWidth >= minCardWidth) break;
        next -= 1;
      }

      setEffectiveColumns((prev) => (prev === next ? prev : next));
    };

    compute();
    const observer = new ResizeObserver(compute);
    observer.observe(listEl);
    return () => observer.disconnect();
  }, [layoutMode]);

  async function pickAddRepo() {
    setError(null);
    const dir = await open({
      directory: true,
      multiple: false,
      defaultPath: getEffectiveRepoRoot() || resolvedDefaultRoot || undefined,
    });
    if (dir === null || Array.isArray(dir)) return;
    setBusy(true);
    try {
      await hubAddRepo(dir);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function pickScan() {
    setError(null);
    const dir = await open({
      directory: true,
      multiple: false,
      defaultPath: getEffectiveRepoRoot() || resolvedDefaultRoot || undefined,
    });
    if (dir === null || Array.isArray(dir)) return;
    setBusy(true);
    try {
      await hubScanDirectory(dir, 12);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function syncHeads() {
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const s = await hubRefreshHeads();
      await refresh();
      const parts = [ui.checkedTotal(s.total), ui.headUpdated(s.ok)];
      if (s.failed > 0) parts.push(ui.headFailed(s.failed));
      setNotice(parts.join(" · "));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function pickZip() {
    setError(null);
    const file = await open({
      multiple: false,
      filters: [{ name: "Zip", extensions: ["zip"] }],
    });
    if (file === null || Array.isArray(file)) return;
    setBusy(true);
    try {
      await importZip(file, getEffectiveRepoRoot() || null);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function submitCloneRepo() {
    const url = cloneUrl.trim();
    if (!url) return;
    setError(null);
    setCloneLog([]);
    setCloneResult(null);
    setBusy(true);
    try {
      const sid = await hubCloneRepoStream(url, getEffectiveRepoRoot() || null);
      setCloneSessionId(sid);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  async function cancelClone() {
    if (cloneSessionId) {
      await hubCancelClone(cloneSessionId);
    }
    setBusy(false);
    setCloneSessionId(null);
    setCloneLog([]);
    setCloneResult(null);
  }

  async function toggleFavorite(r: RepoRecord) {
    try {
      await hubSetFavorite(r.id, !r.isFavorite);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function openRepo(r: RepoRecord) {
    try {
      await hubTouchRepo(r.id);
    } catch {
    }
    onOpenRepo(r);
  }

  return (
    <div
      className={`hub-view hub-view-${layoutMode} hub-columns-auto`}
      style={
        {
          ["--hub-effective-columns" as string]: String(effectiveColumns),
        } as CSSProperties
      }
    >
      <header className="hub-toolbar">
        <input
          className="search-input"
          placeholder={isZh ? "搜索路径、名称或标签…" : "Search path, name, or tags..."}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <label className="hub-sort-control">
          <span className="sr-only">{isZh ? "排序方式" : "Sort mode"}</span>
          <select
            className="hub-sort-select"
            value={sortMode}
            onChange={(e) => setSortMode(e.target.value as SortMode)}
            aria-label={isZh ? "排序方式" : "Sort mode"}
          >
            {sortOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </label>
        <label className="hub-favorites-only">
          <input
            type="checkbox"
            checked={favoritesOnly}
            onChange={(e) => setFavoritesOnly(e.target.checked)}
          />
          <span>{isZh ? "只看收藏" : "Favorites only"}</span>
        </label>
        <div className="hub-toolbar-actions" ref={moreRef}>
          <button
            type="button"
            className="hub-btn-primary btn-primary"
            onClick={() => void pickAddRepo()}
            disabled={busy}
          >
            {isZh ? "添加仓库" : "Add Repo"}
          </button>
          <button
            type="button"
            className="btn-secondary"
            onClick={() => setCloneOpen(true)}
            disabled={busy}
          >
            {isZh ? "远程克隆" : "Clone"}
          </button>
          <div className="hub-more">
            <button
              type="button"
              className="hub-more-trigger btn-secondary"
              aria-expanded={moreOpen}
              aria-haspopup="menu"
              aria-label={ui.moreActions}
              onClick={() => setMoreOpen((o) => !o)}
              disabled={busy}
            >
              ⋯
            </button>
            {moreOpen && (
              <div className="hub-dropdown" role="menu">
                <button
                  type="button"
                  role="menuitem"
                  disabled={busy}
                  onClick={() => {
                    setMoreOpen(false);
                    void pickScan();
                  }}
                >
                  {isZh ? "扫描目录并添加…" : "Scan and add from folder..."}
                </button>
                <button
                  type="button"
                  role="menuitem"
                  disabled={busy}
                  onClick={() => {
                    setMoreOpen(false);
                    void pickZip();
                  }}
                >
                  {isZh ? "从 ZIP 导入…" : "Import from ZIP..."}
                </button>
                <div className="hub-dropdown-sep" role="separator" />
                <button
                  type="button"
                  role="menuitem"
                  disabled={busy}
                  onClick={() => {
                    setMoreOpen(false);
                    void refresh();
                  }}
                >
                  {isZh ? "刷新列表" : "Refresh List"}
                </button>
                <button
                  type="button"
                  role="menuitem"
                  disabled={busy}
                  onClick={() => {
                    setMoreOpen(false);
                    void syncHeads();
                  }}
                >
                  {isZh ? "同步全部 HEAD" : "Sync All HEADs"}
                </button>
              </div>
            )}
          </div>
        </div>
      </header>
      {error && <div className="error-banner">{error}</div>}
      {notice && !error && <div className="info-banner">{notice}</div>}
      {cloneOpen && (
        <section
          className="settings-confirm-panel hub-clone-panel"
          role="dialog"
          aria-modal="true"
          aria-live="polite"
        >
          <h4>{isZh ? "远程克隆仓库" : "Clone remote repository"}</h4>
          <p>{isZh ? "仅支持公开 HTTPS 仓库地址。" : "Only public HTTPS repository URLs are supported."}</p>
          <label className="settings-item-label" htmlFor="hub-clone-url-input">
            {isZh ? "仓库 URL" : "Repository URL"}
          </label>
          <input
            id="hub-clone-url-input"
            type="url"
            className="settings-delete-gate-input"
            placeholder="https://github.com/owner/repo.git"
            value={cloneUrl}
            onChange={(e) => setCloneUrl(e.target.value)}
            disabled={busy}
          />
          <p className="settings-note">
            {isZh ? "落地目录：" : "Destination root: "}
              <code>{getEffectiveRepoRoot() || resolvedDefaultRoot || (isZh ? "默认" : "default")}</code>
          </p>
          {cloneLog.length > 0 && (
            <pre ref={cloneLogRef} className="hub-clone-log">{cloneLog.join("\n")}</pre>
          )}
          {cloneResult && !cloneResult.ok && (
            <div className="error-banner">{cloneResult.error}</div>
          )}
          {cloneResult?.ok && (
            <div className="info-banner">{isZh ? "克隆完成" : "Clone complete"}</div>
          )}
          <div className="settings-confirm-actions">
            {busy ? (
              <button
                type="button"
                className="btn-secondary"
                onClick={() => void cancelClone()}
              >
                {isZh ? "取消克隆" : "Cancel clone"}
              </button>
            ) : (
              <button
                type="button"
                className="btn-secondary"
                onClick={() => {
                  setCloneOpen(false);
                  setCloneUrl("");
                  setCloneLog([]);
                  setCloneResult(null);
                  setCloneSessionId(null);
                }}
              >
                {isZh ? "关闭" : "Close"}
              </button>
            )}
            <button
              type="button"
              className="btn-primary"
              onClick={() => void submitCloneRepo()}
              disabled={busy || cloneUrl.trim().length === 0}
            >
              {busy ? (isZh ? "克隆中…" : "Cloning...") : isZh ? "开始克隆" : "Clone"}
            </button>
          </div>
        </section>
      )}
      <ul className="repo-list" ref={listRef}>
        {displayedRepos.map((r) => (
          <li key={r.id} className="repo-card">
            <div className="repo-card-head">
              <div className="repo-card-main">
                <button
                  type="button"
                  className="star-btn"
                  title={r.isFavorite ? ui.unstar : ui.star}
                  onClick={() => void toggleFavorite(r)}
                >
                  {r.isFavorite ? "★" : "☆"}
                </button>
                <button
                  type="button"
                  className="repo-title btn-quiet"
                  onClick={() => void openRepo(r)}
                >
                  {r.displayName ?? r.path.split(/[/\\]/).filter(Boolean).pop()}
                </button>
              </div>
              <div className="repo-head-badges">
                {r.isBare && <span className="badge">bare</span>}
                <button
                  type="button"
                  className="repo-open-btn btn-secondary"
                  onClick={() => void openRepo(r)}
                >
                  {isZh ? "打开" : "Open"}
                </button>
              </div>
            </div>

            <div className="repo-path repo-desc" title={r.projectIntro ?? r.path}>
              {r.projectIntro?.trim() || r.path.split(/[/\\]/).slice(-3).join("/")}
            </div>

            <div className="repo-meta-row">
              <span className="repo-meta-item">
                ⎇ {r.lastHead ? r.lastHead.slice(0, 7) : ui.noCommit}
              </span>
              <span className="repo-meta-item">
                {r.tags.length > 0 ? ui.tagsCount(r.tags.length) : ui.noTags}
              </span>
            </div>

            {r.tags.length > 0 && (
              <div className="repo-tags repo-tags-preview">
                {r.tags.slice(0, 3).map((t) => (
                  <span key={t} className="tag-pill">
                    {t}
                  </span>
                ))}
                {r.tags.length > 3 && (
                  <span className="tag-pill">+{r.tags.length - 3}</span>
                )}
              </div>
            )}
          </li>
        ))}
      </ul>
      {displayedRepos.length === 0 && !error && (
        favoritesOnly ? (
          <p className="empty-hint">
            {isZh ? "没有符合条件的收藏仓库。" : "No matching favorite repositories."}
          </p>
        ) : (
          <div className="hub-onboarding">
            <h3 className="hub-onboarding-title">
              {isZh ? "开始使用 Deskvio" : "Get started with Deskvio"}
            </h3>
            <p className="hub-onboarding-desc">
              {isZh
                ? "选择一种方式添加你的第一个 Git 仓库："
                : "Choose a way to add your first Git repository:"}
            </p>
            <div className="hub-onboarding-cards">
              <button
                type="button"
                className="hub-onboarding-card hub-onboarding-card-primary"
                disabled={busy}
                onClick={() => setCloneOpen(true)}
              >
                <span className="hub-onboarding-card-icon" aria-hidden>↓</span>
                <span className="hub-onboarding-card-title">
                  {isZh ? "克隆远程仓库" : "Clone a remote repo"}
                </span>
                <span className="hub-onboarding-card-desc">
                  {isZh
                    ? "输入 GitHub 等公开仓库的 HTTPS 链接，自动下载到本地。"
                    : "Paste an HTTPS URL from GitHub or other hosts to download a repo locally."}
                </span>
              </button>
              <button
                type="button"
                className="hub-onboarding-card"
                disabled={busy}
                onClick={() => void pickAddRepo()}
              >
                <span className="hub-onboarding-card-icon" aria-hidden>+</span>
                <span className="hub-onboarding-card-title">
                  {isZh ? "添加本地仓库" : "Add a local repo"}
                </span>
                <span className="hub-onboarding-card-desc">
                  {isZh
                    ? "选择电脑上已有的 Git 仓库文件夹。"
                    : "Select an existing Git repository folder on your computer."}
                </span>
              </button>
              <button
                type="button"
                className="hub-onboarding-card"
                disabled={busy}
                onClick={() => void pickScan()}
              >
                <span className="hub-onboarding-card-icon" aria-hidden>⊕</span>
                <span className="hub-onboarding-card-title">
                  {isZh ? "扫描目录" : "Scan a directory"}
                </span>
                <span className="hub-onboarding-card-desc">
                  {isZh
                    ? "选择一个根目录，自动发现其中的所有 Git 仓库。"
                    : "Select a root folder to automatically discover all Git repos within it."}
                </span>
              </button>
            </div>
          </div>
        )
      )}
    </div>
  );
}
