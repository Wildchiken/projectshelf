type Props = {
  locale?: "zh-CN" | "en-US";
};

export function HelpView({ locale = "zh-CN" }: Props) {
  const isZh = locale === "zh-CN";
  return (
    <div className="help-view help-redesign">
      <header className="help-hero">
        <h2>{isZh ? "帮助中心" : "Help Center"}</h2>
        <p className="help-lead">
          {isZh
            ? "Deskvio（仓维）是一款本地优先、无需账号的 Git 仓库管理工具。"
            : "Deskvio is a local-first Git repository manager — no account required."}
        </p>
      </header>

      <section className="help-card">
        <h3>{isZh ? "如何添加你的第一个仓库" : "How to add your first repository"}</h3>
        <ol className="help-steps">
          <li>
            <strong>{isZh ? "克隆远程仓库" : "Clone a remote repo (recommended for beginners)"}</strong>
            <p>
              {isZh
                ? "在门户页面点击「远程克隆」按钮，粘贴 GitHub 等网站的 HTTPS 仓库链接，点击「开始克隆」即可。"
                : "Click the \"Clone\" button on the Hub page, paste an HTTPS repository URL from GitHub or similar, then click \"Clone\"."}
            </p>
          </li>
          <li>
            <strong>{isZh ? "添加本地仓库" : "Add a local repository"}</strong>
            <p>
              {isZh
                ? "点击「添加仓库」，选择电脑上已有的 Git 仓库文件夹（包含 .git 目录的文件夹）。"
                : "Click \"Add Repo\" and pick an existing folder on your computer that contains a .git directory."}
            </p>
          </li>
          <li>
            <strong>{isZh ? "批量扫描" : "Bulk scan"}</strong>
            <p>
              {isZh
                ? "点击工具栏的「⋯」→「扫描目录并添加」，选择一个根目录，Deskvio 会自动发现其中的所有仓库。"
                : "Click \"⋯\" → \"Scan and add from folder\", choose a root directory, and Deskvio will find all repos inside it."}
            </p>
          </li>
        </ol>
      </section>

      <section className="help-card">
        <h3>{isZh ? "代码页快捷键" : "Code tab shortcuts"}</h3>
        <ul className="help-steps">
          <li>
            <strong>
              <kbd className="help-kbd">t</kbd>
              {isZh ? " 转到文件" : " Go to file"}
            </strong>
            <p>
              {isZh
                ? "在「代码」标签下（未全屏阅读时）按 t，可全仓库模糊搜索路径并打开文件，类似 GitHub。"
                : "On the Code tab (when not in full-screen reader), press t to fuzzy-search file paths repo-wide and open a file — similar to GitHub."}
            </p>
          </li>
          <li>
            <strong>
              <kbd className="help-kbd">Esc</kbd>
              {isZh ? " 退出 Markdown 全屏阅读" : " Exit full-screen Markdown reader"}
            </strong>
            <p>
              {isZh
                ? "在阅读模式下按 Esc 可退出全屏并回到分栏视图。"
                : "While reading Markdown in immersive mode, press Esc to return to the split view."}
            </p>
          </li>
        </ul>
      </section>

      <section className="help-card">
        <h3>{isZh ? "什么是 Git 仓库？" : "What is a Git repository?"}</h3>
        <p>
          {isZh
            ? "Git 仓库是一个由 Git 版本控制系统跟踪的项目文件夹。它包含一个隐藏的 .git 目录，其中保存了所有历史版本和变更记录。你可以在 GitHub、GitLab 等平台找到公开的仓库。"
            : "A Git repository is a project folder tracked by the Git version control system. It contains a hidden .git directory that stores the full history of all changes. You can find public repositories on platforms like GitHub and GitLab."}
        </p>
      </section>

      <section className="help-grid">
        <article className="help-card">
          <h3>{isZh ? "常见问题" : "FAQ"}</h3>
          <dl className="help-faq">
            <dt>{isZh ? "克隆的仓库存在哪里？" : "Where are cloned repos stored?"}</dt>
            <dd>
              {isZh
                ? "默认存储在你的用户目录下的 Deskvio 文件夹。你可以在「设置」→「仓库根目录」中修改。"
                : "By default, in the Deskvio folder under your home directory. You can change this in Settings → Repository root directory."}
            </dd>
            <dt>{isZh ? "支持 SSH 或私有仓库吗？" : "Does it support SSH or private repos?"}</dt>
            <dd>
              {isZh
                ? "目前仅支持公开的 HTTPS 仓库链接。SSH 和私有仓库暂不支持。"
                : "Currently only public HTTPS URLs are supported. SSH and private repos are not yet available."}
            </dd>
            <dt>{isZh ? "如何更换主题？" : "How do I change the theme?"}</dt>
            <dd>
              {isZh
                ? "点击侧栏底部的「设置」（⋯），在「显示与主题」中切换浅色/深色。"
                : "Click \"Settings\" (⋯) at the bottom of the sidebar, then toggle Light/Dark under \"Display & Theme\"."}
            </dd>
            <dt>{isZh ? "项目简介和标签存在哪？" : "Where are project intro and tags stored?"}</dt>
            <dd>
              {isZh
                ? "除本机数据库外，会写入仓库目录下的 .deskvio/project.json。复制整个仓库文件夹即可带走。若希望 git clone 后也有同样内容，请将该文件纳入 Git；若不想进版本库，可把 .deskvio/ 加入 .gitignore。"
                : "Besides the local database, they are written to .deskvio/project.json inside the repository. Copy the whole repo folder to keep them. Commit that file if you want the same data after git clone; add .deskvio/ to .gitignore to keep them local-only."}
            </dd>
            <dt>{isZh ? "如何用系统程序打开当前文件？" : "How do I open the current file in a system app?"}</dt>
            <dd>
              {isZh
                ? "在「代码」标签选中文件后，使用「用系统应用打开」或「在文件夹中显示」（裸仓库无工作区时不可用）。"
                : "On the Code tab, after selecting a file, use \"Open in default app\" or \"Reveal in folder\" (not available for bare repos without a worktree)."}
            </dd>
          </dl>
        </article>

        <article className="help-card">
          <h3>{isZh ? "开发者信息" : "For Developers"}</h3>
          <pre className="help-code">{`npm install
npm run tauri dev`}</pre>
          <p className="help-note">
            {isZh ? "构建发布：" : "Build for production: "}<code>npm run tauri build</code>
          </p>
          <p className="help-note">
            {isZh ? "依赖" : "Requires"}{" "}
            <a href="https://nodejs.org/" target="_blank" rel="noreferrer">Node.js</a>{" + "}
            <a href="https://rustup.rs/" target="_blank" rel="noreferrer">Rust</a>{" + "}
            <a href="https://v2.tauri.app/start/prerequisites/" target="_blank" rel="noreferrer">
              {isZh ? "Tauri 前置环境" : "Tauri prerequisites"}
            </a>
            {isZh ? "。" : "."}
          </p>
        </article>
      </section>
    </div>
  );
}
