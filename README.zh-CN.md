# 仓维（Deskvio）

[English](./README.md)

仓维（Deskvio）是一个本地优先的 Git 桌面工具，用来管理你的个人项目仓库。

它提供一个更直接的本地使用方式：不用登录，不用启动后端服务，也不用依赖终端命令，就可以查看仓库、浏览提交、检查改动，并维护仓库级 Releases 备份。

## 特性

- 本地优先，离线可用
- 无需登录或自建服务
- 多仓库统一管理
- 提供代码、提交和改动视图
- 内置仓库级 Releases 版本文件备份

## 功能

- 在一个界面中打开和管理多个本地 Git 仓库
- 浏览文件和仓库内容
- 查看提交历史、引用、远程和工作区改动
- 完成轻量本地提交
- 为仓库维护带元数据和附件的 release 记录

## Releases

仓维（Deskvio）中的 Releases 定位是本地备份管理，而不是 CI/CD 发布流程。

- 一个仓库可以维护多个 releases
- 每个 release 可以包含元数据和多个资产文件
- 元数据保存在 `.deskvio/releases/releases.json`
- 文件保存在 `.deskvio/releases/assets/...`

适合保存导出包、交付文件、压缩档案，或其他和某个版本对应的本地文件。

## 数据与隐私

- 应用级数据保存在本机
- Release 元数据和资产保存在对应仓库内
- 不依赖账号或远程服务

## 平台支持

- macOS
- Windows
- Linux

具体可用性取决于本机构建环境和 Tauri 前置依赖。

## 开发

```bash
npm install
npm run tauri dev
```

依赖：

- [Rust](https://rustup.rs/)
- [Tauri 前置依赖](https://tauri.app/start/prerequisites/)

## 构建

```bash
npm run tauri build
```

## 便携 Git

便携 Git 方案见 [bundled-git/README.md](./bundled-git/README.md)。

## 许可证

- [MIT](./LICENSE)
- [第三方许可证清单](./THIRD_PARTY_LICENSES.md)
