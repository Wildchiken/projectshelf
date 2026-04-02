import { useLayoutEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  hubListRepos,
  hubRemoveRepo,
  hubUnlinkRepo,
  appDbStatus,
  type RepoRecord,
} from "./api";
import { HelpView } from "./HelpView";
import { HubView } from "./HubView";
import { RepoView } from "./RepoView";
import { SettingsPanel, type AppLocale } from "./SettingsPanel";
import "./App.css";

const THEME_KEY = "deskvio-theme";
const HUB_LAYOUT_KEY = "deskvio-layout";
const APP_LOCALE_KEY = "deskvio-locale";
const APP_MOTION_KEY = "deskvio-motion-enabled";
const APP_REPO_ROOT_KEY = "deskvio-repo-root";
const APP_REPO_ROOT_ONBOARDING_KEY = "deskvio-repo-root-onboarding-v2";
const APP_SIDEBAR_COLLAPSED_KEY = "deskvio-sidebar-collapsed";

type Theme = "light" | "dark";
type HubLayoutMode = "comfortable" | "compact";

function readStoredTheme(): Theme {
  if (typeof window === "undefined") return "light";
  const s = localStorage.getItem(THEME_KEY);
  if (s === "light" || s === "dark") return s;
  return "light";
}

type Screen = "hub" | "help" | "repo";
type ResetMode = "unlink" | "delete";

type ResetProgress = {
  mode: ResetMode;
  total: number;
  done: number;
  success: number;
  failed: number;
};

type InlineNotice = {
  tone: "success" | "error" | "info";
  text: string;
};

function readStoredHubLayout(): HubLayoutMode {
  if (typeof window === "undefined") return "comfortable";
  const s = localStorage.getItem(HUB_LAYOUT_KEY);
  if (s === "comfortable" || s === "compact") return s;
  return "comfortable";
}

function readStoredLocale(): AppLocale {
  if (typeof window === "undefined") return "zh-CN";
  const s = localStorage.getItem(APP_LOCALE_KEY);
  if (s === "zh-CN" || s === "en-US") return s;
  return "zh-CN";
}

function readStoredMotionEnabled(): boolean {
  if (typeof window === "undefined") return true;
  return localStorage.getItem(APP_MOTION_KEY) !== "false";
}

function readStoredRepoRoot(): string {
  if (typeof window === "undefined") return "";
  return localStorage.getItem(APP_REPO_ROOT_KEY) ?? "";
}

function readStoredSidebarCollapsed(): boolean {
  if (typeof window === "undefined") return false;
  return localStorage.getItem(APP_SIDEBAR_COLLAPSED_KEY) === "true";
}

function App() {
  const [screen, setScreen] = useState<Screen>("hub");
  const [activeRepo, setActiveRepo] = useState<RepoRecord | null>(null);
  const [theme, setTheme] = useState<Theme>(() => readStoredTheme());
  const [hubLayoutMode, setHubLayoutMode] = useState<HubLayoutMode>(() =>
    readStoredHubLayout(),
  );
  const [locale, setLocale] = useState<AppLocale>(() => readStoredLocale());
  const [motionEnabled, setMotionEnabled] = useState<boolean>(() =>
    readStoredMotionEnabled(),
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hubRefreshToken, setHubRefreshToken] = useState(0);
  const [repoRoot, setRepoRoot] = useState<string>(() => readStoredRepoRoot());
  const [showRepoRootOnboarding, setShowRepoRootOnboarding] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState<boolean>(() =>
    readStoredSidebarCollapsed(),
  );
  const [notice, setNotice] = useState<InlineNotice | null>(null);
  const noticeTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [resetProgress, setResetProgress] = useState<ResetProgress | null>(null);
  const [resetSummary, setResetSummary] = useState<ResetProgress | null>(null);

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  useLayoutEffect(() => {
    localStorage.setItem(HUB_LAYOUT_KEY, hubLayoutMode);
  }, [hubLayoutMode]);

  useLayoutEffect(() => {
    localStorage.setItem(APP_LOCALE_KEY, locale);
  }, [locale]);

  useLayoutEffect(() => {
    localStorage.setItem(APP_MOTION_KEY, motionEnabled ? "true" : "false");
    document.documentElement.dataset.motion = motionEnabled ? "on" : "off";
  }, [motionEnabled]);

  useLayoutEffect(() => {
    localStorage.setItem(APP_REPO_ROOT_KEY, repoRoot);
  }, [repoRoot]);

  useLayoutEffect(() => {
    localStorage.setItem(APP_SIDEBAR_COLLAPSED_KEY, sidebarCollapsed ? "true" : "false");
  }, [sidebarCollapsed]);

  useLayoutEffect(() => {
    const savedRoot = readStoredRepoRoot().trim();
    const onboarded = localStorage.getItem(APP_REPO_ROOT_ONBOARDING_KEY) === "done";
    if (!onboarded && savedRoot.length === 0) setShowRepoRootOnboarding(true);
  }, []);

  const isZh = locale === "zh-CN";

  useLayoutEffect(() => {
    void (async () => {
      try {
        const st = await appDbStatus();
        if (st.status === "ok") return;
        setNotice({
          tone: "info",
          text:
            st.status === "temp"
              ? isZh
                ? "数据库不可写，已切换到临时数据库。重启后可能丢失仓库列表/缓存。"
                : "Database is not writable; switched to a temporary database. Repo list/cache may be lost after restart."
              : isZh
                ? "检测到数据库不可写，已尝试自动修复/重建。重启后可能丢失仓库列表/缓存。"
                : "Database is not writable; attempted auto repair/rebuild. Repo list/cache may be lost after restart.",
        });
      } catch {
      }
    })();
  }, [isZh]);

  useLayoutEffect(() => {
    if (!notice) return;
    if (notice.tone === "error") return;

    if (noticeTimeoutRef.current) clearTimeout(noticeTimeoutRef.current);
    noticeTimeoutRef.current = setTimeout(() => setNotice(null), 8000);

    return () => {
      if (noticeTimeoutRef.current) clearTimeout(noticeTimeoutRef.current);
      noticeTimeoutRef.current = null;
    };
  }, [notice]);

  const labels = isZh
    ? {
        hub: "门户",
        help: "帮助",
        settings: "设置",
      }
    : {
        hub: "Hub",
        help: "Help",
        settings: "Settings",
      };
  const sidebarLabels = isZh
    ? {
        collapse: "收起侧栏",
        expand: "展开侧栏",
      }
    : {
        collapse: "Collapse sidebar",
        expand: "Expand sidebar",
      };

  const onboardingLabels = isZh
    ? {
        title: "指定仓库根目录",
        desc: "你需要先选择一个可见目录。之后克隆与 ZIP 导入会落在这里。",
      }
    : {
        title: "Choose repository root",
        desc: "Select a visible directory first. Future clones and ZIP imports will land here.",
      };

  async function pickRepoRoot(): Promise<string | null> {
    const dir = await open({ directory: true, multiple: false });
    if (dir === null || Array.isArray(dir)) return null;
    const next = dir.trim();
    return next.length > 0 ? next : null;
  }

  async function confirmRepoRootOnboarding() {
    const nextRoot = await pickRepoRoot();
    if (!nextRoot) return;
    setRepoRoot(nextRoot);
    localStorage.setItem(APP_REPO_ROOT_ONBOARDING_KEY, "done");
    setShowRepoRootOnboarding(false);
  }

  async function runHubReset(mode: ResetMode) {
    const isUnlink = mode === "unlink";
    const action = isUnlink ? hubUnlinkRepo : hubRemoveRepo;
    setNotice(null);
    setResetSummary(null);
    const list = await hubListRepos();
    const total = list.length;
    const progress: ResetProgress = {
      mode,
      total,
      done: 0,
      success: 0,
      failed: 0,
    };
    setResetProgress(progress);
    if (total === 0) {
      setResetProgress(null);
      setResetSummary(progress);
      setNotice({
        tone: "info",
        text: isZh
          ? "没有可重置的仓库记录。"
          : "No repository records to reset.",
      });
      return;
    }
    const chunkSize = 6;
    let done = 0;
    let success = 0;
    let failed = 0;
    for (let i = 0; i < list.length; i += chunkSize) {
      const batch = list.slice(i, i + chunkSize);
      await Promise.allSettled(
        batch.map(async (repo) => {
          try {
            await action(repo.id);
            success += 1;
          } catch {
            failed += 1;
          } finally {
            done += 1;
            setResetProgress({
              mode,
              total,
              done,
              success,
              failed,
            });
          }
        }),
      );
    }
    setActiveRepo(null);
    setScreen("hub");
    setHubRefreshToken((n) => n + 1);
    const finalProgress: ResetProgress = { mode, total, done, success, failed };
    setResetProgress(null);
    setResetSummary(finalProgress);
    if (failed === 0) {
      setNotice({
        tone: "success",
        text: isZh
          ? `重置完成：成功 ${success}/${total}`
          : `Reset complete: ${success}/${total} succeeded`,
      });
    } else {
      setNotice({
        tone: "error",
        text: isZh
          ? `重置完成：成功 ${success}/${total}，失败 ${failed}`
          : `Reset complete: ${success}/${total} succeeded, ${failed} failed`,
      });
    }
  }

  async function resetHubUnlinkOnly() {
    try {
      await runHubReset("unlink");
    } catch (e) {
      setResetProgress(null);
      setNotice({
        tone: "error",
        text: (isZh ? "普通重置失败：" : "Standard reset failed: ") + String(e),
      });
    }
  }

  async function resetHubDeleteAll() {
    try {
      await runHubReset("delete");
    } catch (e) {
      setResetProgress(null);
      setNotice({
        tone: "error",
        text: (isZh ? "危险删除失败：" : "Dangerous delete failed: ") + String(e),
      });
    }
  }

  return (
    <div className={`app-shell ${sidebarCollapsed ? "sidebar-collapsed" : ""}`}>
      <aside className="sidebar">
        <div className="sidebar-head">
          <div className="brand">{sidebarCollapsed ? "DV" : "Deskvio"}</div>
          <button
            type="button"
            className="sidebar-toggle-btn"
            aria-label={sidebarCollapsed ? sidebarLabels.expand : sidebarLabels.collapse}
            title={sidebarCollapsed ? sidebarLabels.expand : sidebarLabels.collapse}
            onClick={() => setSidebarCollapsed((v) => !v)}
          >
            {sidebarCollapsed ? "»" : "«"}
          </button>
        </div>
        <nav>
          <button
            type="button"
            className={screen === "hub" ? "nav-active" : ""}
            aria-label={labels.hub}
            onClick={() => {
              setScreen("hub");
              setActiveRepo(null);
            }}
          >
            <span className="sidebar-nav-icon" aria-hidden="true">
              ⌂
            </span>
            <span className="sidebar-nav-label">{labels.hub}</span>
          </button>
          <button
            type="button"
            className={screen === "help" ? "nav-active" : ""}
            aria-label={labels.help}
            onClick={() => {
              setScreen("help");
              setActiveRepo(null);
            }}
          >
            <span className="sidebar-nav-icon" aria-hidden="true">
              ?
            </span>
            <span className="sidebar-nav-label">{labels.help}</span>
          </button>
        </nav>
        <div className="sidebar-footer">
          <button
            type="button"
            className="settings-entry-btn"
            aria-label={labels.settings}
            onClick={() => setSettingsOpen(true)}
          >
            <span className="sidebar-nav-icon" aria-hidden="true">
              ⋯
            </span>
            <span className="sidebar-nav-label">{labels.settings}</span>
          </button>
        </div>
      </aside>
      <main className="main">
        {notice && (
          <div className={notice.tone === "error" ? "error-banner" : "info-banner"}>
            {notice.text}
          </div>
        )}
        {screen === "hub" && (
          <div className="screen-panel">
            <HubView
              locale={locale}
              layoutMode={hubLayoutMode}
              repoRoot={repoRoot}
              refreshToken={hubRefreshToken}
              onOpenRepo={(r) => {
                setActiveRepo(r);
                setScreen("repo");
              }}
            />
          </div>
        )}
        {screen === "help" && (
          <div className="screen-panel">
            <HelpView locale={locale} />
          </div>
        )}
        {screen === "repo" && activeRepo && (
          <div className="screen-panel">
            <RepoView
              repo={activeRepo}
              locale={locale}
              onUpdateRepo={(updated) => setActiveRepo(updated)}
              onRemoveRepo={() => {
                setActiveRepo(null);
                setScreen("hub");
              }}
              onBack={() => {
                setActiveRepo(null);
                setScreen("hub");
              }}
            />
          </div>
        )}
      </main>
      <SettingsPanel
        open={settingsOpen}
        locale={locale}
        theme={theme}
        hubLayoutMode={hubLayoutMode}
        motionEnabled={motionEnabled}
        repoRoot={repoRoot}
        onClose={() => setSettingsOpen(false)}
        onLocaleChange={setLocale}
        onHubLayoutModeChange={setHubLayoutMode}
        onToggleTheme={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
        onMotionEnabledChange={setMotionEnabled}
        onRepoRootChange={setRepoRoot}
        onResetRepoRoot={() => {
          void (async () => {
            const nextRoot = await pickRepoRoot();
            if (nextRoot) setRepoRoot(nextRoot);
          })();
        }}
        onResetUnlinkOnly={resetHubUnlinkOnly}
        onResetDeleteAll={resetHubDeleteAll}
        resetProgress={resetProgress}
        resetSummary={resetSummary}
      />
      {showRepoRootOnboarding && (
        <div className="settings-modal-backdrop" role="presentation">
          <section className="settings-modal app-onboarding-modal" role="dialog" aria-modal="true">
            <header className="settings-modal-head">
              <h2>{onboardingLabels.title}</h2>
            </header>
            <section className="settings-card">
              <p className="settings-note">{onboardingLabels.desc}</p>
              <div className="settings-confirm-actions">
                <button
                  type="button"
                  className="btn-primary"
                  onClick={() => void confirmRepoRootOnboarding()}
                >
                  {isZh ? "选择目录" : "Select directory"}
                </button>
              </div>
            </section>
          </section>
        </div>
      )}
    </div>
  );
}

export default App;
