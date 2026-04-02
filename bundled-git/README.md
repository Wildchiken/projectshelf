# 便携版 Git 可执行文件

Deskvio（仓维）默认使用系统 PATH 中的 `git`。若希望 **U 盘即插即用、不依赖本机已安装 Git**，请将官方 **Portable Git** 解压到应用同目录，或使用环境变量。

## 目录布局（与打包后的可执行文件同级）

**macOS / Linux**

```text
Deskvio.app/   （或你的可执行文件所在目录）
  portable-git/
    bin/
      git
    ...
```

**Windows**

```text
Deskvio.exe
portable-git/
  cmd/
    git.exe
  ...
```

应用会按上述路径自动探测；若路径不同，可设置环境变量：

- `PORTABLE_GIT_PATH`：指向 `git`（或 `git.exe`）的**绝对路径**。

## 获取 Portable Git

- Windows: [Git for Windows](https://git-scm.com/download/win) 安装器中带 **Portable** 版本说明；也可使用第三方便携打包。
- macOS: 通常依赖 Xcode Command Line Tools 或 Homebrew 的 `git`；真正「免安装」可将 `git` 及依赖一并放入 `portable-git`（体积较大，需自行处理库路径与签名）。

## 构建应用

在项目根目录：

```bash
npm install
npm run tauri build
```

产物在 `src-tauri/target/release/bundle/`（随平台变化）。将 `portable-git` 与可执行文件放在同一目录即可分发。
