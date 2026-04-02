# Deskvio

[中文文档](./README.zh-CN.md)

Deskvio is a local-first desktop app for managing personal Git repositories.

It gives you a simple way to browse code, inspect commits, review changes, and keep repository-level release backups without signing in, running a server, or depending on terminal workflows.

## Highlights

- Local-first, offline-friendly workflow
- No login or self-hosted backend required
- Multi-repository hub for personal projects
- Built-in code, commit, and change views
- Repository-level Releases for versioned file backups

## What It Does

- Open and manage multiple local Git repositories in one place
- Browse files and read repository contents with a desktop UI
- Inspect commit history, refs, remotes, and working tree changes
- Create lightweight local commits
- Maintain release entries with metadata and attached assets

## Releases

In Deskvio, Releases are designed as a local backup manager rather than a CI/CD publishing workflow.

- Each repository can keep multiple releases
- Each release can include metadata and multiple assets
- Release metadata is stored in `.deskvio/releases/releases.json`
- Release files are stored in `.deskvio/releases/assets/...`

This is useful for keeping exported builds, deliverables, archives, or version-specific files next to the repository they belong to.

## Data and Privacy

- App-level data is stored locally on your machine
- Release metadata and assets are stored inside each repository
- No account or remote service is required

## Platform Support

- macOS
- Windows
- Linux

Platform availability depends on local build environment and Tauri prerequisites.

## Development

```bash
npm install
npm run tauri dev
```

Requirements:

- [Rust](https://rustup.rs/)
- [Tauri prerequisites](https://tauri.app/start/prerequisites/)

## Build

```bash
npm run tauri build
```

## Portable Git

For portable Git setup, see [bundled-git/README.md](./bundled-git/README.md).

## License

- [MIT](./LICENSE)
- [Third-party licenses](./THIRD_PARTY_LICENSES.md)
