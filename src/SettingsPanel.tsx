import { useEffect, useId, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

type Theme = "light" | "dark";
type HubLayoutMode = "comfortable" | "compact";
export type AppLocale = "zh-CN" | "en-US";
type ResetFlow = "unlink" | "deleteConfirm" | "deleteType";
type ResetMode = "unlink" | "delete";
type ResetProgress = {
  mode: ResetMode;
  total: number;
  done: number;
  success: number;
  failed: number;
};

type Props = {
  open: boolean;
  locale: AppLocale;
  theme: Theme;
  hubLayoutMode: HubLayoutMode;
  motionEnabled: boolean;
  repoRoot: string;
  onClose: () => void;
  onLocaleChange: (locale: AppLocale) => void;
  onHubLayoutModeChange: (mode: HubLayoutMode) => void;
  onToggleTheme: () => void;
  onMotionEnabledChange: (enabled: boolean) => void;
  onRepoRootChange: (path: string) => void;
  onResetRepoRoot: () => void;
  onResetUnlinkOnly: () => Promise<void>;
  onResetDeleteAll: () => Promise<void>;
  resetProgress: ResetProgress | null;
  resetSummary: ResetProgress | null;
};

const TEXT = {
  "zh-CN": {
    title: "设置",
    close: "关闭",
    display: "显示与主题",
    density: "显示密度",
    comfortable: "平衡",
    compact: "紧凑",
    theme: "主题",
    toLight: "切到浅色",
    toDark: "切到深色",
    language: "语言",
    zh: "中文",
    en: "English",
    animation: "动画",
    animationOn: "开启动画",
    animationOff: "禁用动画",
    reset: "仓库重置",
    resetHint: "普通重置仅清空门户记录；危险删除会同时删除磁盘目录。",
    unlinkOnly: "普通重置（不删磁盘）",
    deleteAll: "危险删除（删磁盘）",
    busy: "处理中…",
    cancel: "取消",
    continue: "继续",
    confirmResetTitle: "确认普通重置",
    confirmResetBody: "仅清空 Hub 记录，不删除磁盘仓库目录。确认继续？",
    confirmDeleteTitle: "确认危险删除",
    confirmDeleteBody: "此操作会删除 Hub 记录并删除磁盘仓库目录。",
    typedDeleteHint: "请输入 DELETE 以继续",
    typedDeleteLabel: "输入确认",
    typedDeletePlaceholder: "DELETE",
    typedDeleteMismatch: "输入必须为 DELETE",
    progressRunning: "处理中",
    progressResult: "结果",
    success: "成功",
    failed: "失败",
    repoRoot: "仓库根目录",
    repoRootHint: "新导入/克隆默认落到此目录。",
    repoRootPlaceholder: "默认仓库根目录",
    restoreDefaultRoot: "恢复默认目录",
    browse: "浏览…",
  },
  "en-US": {
    title: "Settings",
    close: "Close",
    display: "Display & Theme",
    density: "Density",
    comfortable: "Balanced",
    compact: "Compact",
    theme: "Theme",
    toLight: "Switch to Light",
    toDark: "Switch to Dark",
    language: "Language",
    zh: "Chinese",
    en: "English",
    animation: "Animation",
    animationOn: "Enable animation",
    animationOff: "Disable animation",
    reset: "Repository Reset",
    resetHint: "Standard reset unlinks hub records only; dangerous delete also removes repository directories.",
    unlinkOnly: "Standard reset (keep disk files)",
    deleteAll: "Dangerous delete (remove disk files)",
    busy: "Processing...",
    cancel: "Cancel",
    continue: "Continue",
    confirmResetTitle: "Confirm standard reset",
    confirmResetBody: "This only unlinks Hub records and keeps repository directories on disk. Continue?",
    confirmDeleteTitle: "Confirm dangerous delete",
    confirmDeleteBody: "This action removes Hub records and permanently deletes repository directories.",
    typedDeleteHint: "Type DELETE to continue",
    typedDeleteLabel: "Confirmation input",
    typedDeletePlaceholder: "DELETE",
    typedDeleteMismatch: "Input must equal DELETE",
    progressRunning: "In progress",
    progressResult: "Result",
    success: "Success",
    failed: "Failed",
    repoRoot: "Repository root directory",
    repoRootHint: "New imports and clones will be created under this directory.",
    repoRootPlaceholder: "Default repository root",
    restoreDefaultRoot: "Restore default directory",
    browse: "Browse...",
  },
} as const;

export function SettingsPanel({
  open,
  locale,
  theme,
  hubLayoutMode,
  motionEnabled,
  repoRoot,
  onClose,
  onLocaleChange,
  onHubLayoutModeChange,
  onToggleTheme,
  onMotionEnabledChange,
  onRepoRootChange,
  onResetRepoRoot,
  onResetUnlinkOnly,
  onResetDeleteAll,
  resetProgress,
  resetSummary,
}: Props) {
  const [resetFlow, setResetFlow] = useState<ResetFlow | null>(null);
  const [deleteGateInput, setDeleteGateInput] = useState("");
  const [runningReset, setRunningReset] = useState<"unlink" | "delete" | null>(null);
  const modalRef = useRef<HTMLElement | null>(null);
  const closeBtnRef = useRef<HTMLButtonElement | null>(null);
  const previousActiveRef = useRef<HTMLElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) {
      setResetFlow(null);
      setDeleteGateInput("");
      setRunningReset(null);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    previousActiveRef.current = document.activeElement as HTMLElement | null;
    closeBtnRef.current?.focus();
    return () => {
      previousActiveRef.current?.focus();
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const modal = modalRef.current;
    if (!modal) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && runningReset === null) {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const nodes = modal.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href]',
      );
      if (nodes.length === 0) return;
      const first = nodes[0];
      const last = nodes[nodes.length - 1];
      const current = document.activeElement as HTMLElement | null;
      if (!e.shiftKey && current === last) {
        e.preventDefault();
        first.focus();
      } else if (e.shiftKey && current === first) {
        e.preventDefault();
        last.focus();
      }
    };
    modal.addEventListener("keydown", onKeyDown);
    return () => modal.removeEventListener("keydown", onKeyDown);
  }, [open, onClose, runningReset]);

  if (!open) return null;
  const t = TEXT[locale];
  const deleteGateMatched = deleteGateInput.trim().toUpperCase() === "DELETE";

  async function runUnlinkReset() {
    setRunningReset("unlink");
    try {
      await onResetUnlinkOnly();
      setResetFlow(null);
    } finally {
      setRunningReset(null);
    }
  }

  async function runDangerousReset() {
    if (!deleteGateMatched) return;
    setRunningReset("delete");
    try {
      await onResetDeleteAll();
      setResetFlow(null);
      setDeleteGateInput("");
    } finally {
      setRunningReset(null);
    }
  }

  return (
    <div className="settings-modal-backdrop" role="presentation" onClick={onClose}>
      <section
        ref={modalRef}
        className="settings-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onClick={(e) => e.stopPropagation()}
      >
        <header className="settings-modal-head">
          <h2 id={titleId}>{t.title}</h2>
          <button ref={closeBtnRef} type="button" className="btn-secondary" onClick={onClose}>
            {t.close}
          </button>
        </header>

        <section className="settings-card">
          <h3>{t.display}</h3>
          <label className="settings-item-label">{t.density}</label>
          <div className="settings-segmented">
            <button
              type="button"
              className={hubLayoutMode === "comfortable" ? "active" : ""}
              onClick={() => onHubLayoutModeChange("comfortable")}
            >
              {t.comfortable}
            </button>
            <button
              type="button"
              className={hubLayoutMode === "compact" ? "active" : ""}
              onClick={() => onHubLayoutModeChange("compact")}
            >
              {t.compact}
            </button>
          </div>
          <label className="settings-item-label">{t.theme}</label>
          <button type="button" className="theme-toggle" onClick={onToggleTheme}>
            {theme === "dark" ? t.toLight : t.toDark}
          </button>
        </section>

        <section className="settings-card">
          <h3>{t.language}</h3>
          <div className="settings-segmented">
            <button
              type="button"
              className={locale === "zh-CN" ? "active" : ""}
              onClick={() => onLocaleChange("zh-CN")}
            >
              {t.zh}
            </button>
            <button
              type="button"
              className={locale === "en-US" ? "active" : ""}
              onClick={() => onLocaleChange("en-US")}
            >
              {t.en}
            </button>
          </div>
        </section>

        <section className="settings-card">
          <h3>{t.animation}</h3>
          <div className="settings-segmented">
            <button
              type="button"
              className={motionEnabled ? "active" : ""}
              onClick={() => onMotionEnabledChange(true)}
            >
              {t.animationOn}
            </button>
            <button
              type="button"
              className={!motionEnabled ? "active" : ""}
              onClick={() => onMotionEnabledChange(false)}
            >
              {t.animationOff}
            </button>
          </div>
        </section>

        <section className="settings-card">
          <h3>{t.repoRoot}</h3>
          <p className="settings-note">{t.repoRootHint}</p>
          <div className="settings-repo-root-row">
            <input
              type="text"
              className="settings-delete-gate-input"
              value={repoRoot}
              placeholder={t.repoRootPlaceholder}
              aria-label={t.repoRoot}
              onChange={(e) => onRepoRootChange(e.target.value)}
            />
            <button
              type="button"
              className="btn-secondary"
              onClick={async () => {
                const dir = await openDialog({
                  directory: true,
                  multiple: false,
                  defaultPath: repoRoot || undefined,
                });
                if (typeof dir === "string") onRepoRootChange(dir);
              }}
            >
              {t.browse}
            </button>
          </div>
          <div className="settings-confirm-actions">
            <button type="button" className="btn-secondary" onClick={onResetRepoRoot}>
              {t.restoreDefaultRoot}
            </button>
          </div>
        </section>

        <section className="settings-card settings-danger-card">
          <h3>{t.reset}</h3>
          <p className="settings-note">{t.resetHint}</p>
          <button
            type="button"
            className="btn-secondary"
            onClick={() => setResetFlow("unlink")}
            disabled={runningReset !== null}
          >
            {t.unlinkOnly}
          </button>
          <button
            type="button"
            className="btn-danger"
            onClick={() => setResetFlow("deleteConfirm")}
            disabled={runningReset !== null}
          >
            {t.deleteAll}
          </button>

          {resetFlow === "unlink" && (
            <div className="settings-confirm-panel" role="alertdialog" aria-live="polite">
              <h4>{t.confirmResetTitle}</h4>
              <p>{t.confirmResetBody}</p>
              <div className="settings-confirm-actions">
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={() => setResetFlow(null)}
                  disabled={runningReset !== null}
                >
                  {t.cancel}
                </button>
                <button
                  type="button"
                  className="btn-primary"
                  onClick={() => void runUnlinkReset()}
                  disabled={runningReset !== null}
                >
                  {runningReset === "unlink" ? t.busy : t.continue}
                </button>
              </div>
            </div>
          )}

          {resetFlow === "deleteConfirm" && (
            <div className="settings-confirm-panel settings-confirm-danger" role="alertdialog" aria-live="polite">
              <h4>{t.confirmDeleteTitle}</h4>
              <p>{t.confirmDeleteBody}</p>
              <div className="settings-confirm-actions">
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={() => setResetFlow(null)}
                  disabled={runningReset !== null}
                >
                  {t.cancel}
                </button>
                <button
                  type="button"
                  className="btn-danger"
                  onClick={() => setResetFlow("deleteType")}
                  disabled={runningReset !== null}
                >
                  {t.continue}
                </button>
              </div>
            </div>
          )}

          {resetFlow === "deleteType" && (
            <div className="settings-confirm-panel settings-confirm-danger" role="alertdialog" aria-live="polite">
              <h4>{t.typedDeleteHint}</h4>
              <label className="settings-item-label" htmlFor="settings-delete-gate-input">
                {t.typedDeleteLabel}
              </label>
              <input
                id="settings-delete-gate-input"
                className="settings-delete-gate-input"
                type="text"
                autoComplete="off"
                spellCheck={false}
                value={deleteGateInput}
                onChange={(e) => setDeleteGateInput(e.target.value)}
                placeholder={t.typedDeletePlaceholder}
              />
              {deleteGateInput.length > 0 && !deleteGateMatched && (
                <p className="settings-delete-gate-error" role="alert">
                  {t.typedDeleteMismatch}
                </p>
              )}
              <div className="settings-confirm-actions">
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={() => {
                    setResetFlow(null);
                    setDeleteGateInput("");
                  }}
                  disabled={runningReset !== null}
                >
                  {t.cancel}
                </button>
                <button
                  type="button"
                  className="btn-danger"
                  onClick={() => void runDangerousReset()}
                  disabled={runningReset !== null || !deleteGateMatched}
                >
                  {runningReset === "delete" ? t.busy : t.deleteAll}
                </button>
              </div>
            </div>
          )}
          {resetProgress && (
            <p className="settings-note">
              {t.progressRunning}: {resetProgress.done}/{resetProgress.total} ·
              {" "}{t.success} {resetProgress.success} · {t.failed} {resetProgress.failed}
            </p>
          )}
          {resetSummary && !resetProgress && (
            <p className="settings-note">
              {t.progressResult}: {resetSummary.done}/{resetSummary.total} ·{" "}
              {t.success} {resetSummary.success} · {t.failed} {resetSummary.failed}
            </p>
          )}
        </section>
      </section>
    </div>
  );
}
