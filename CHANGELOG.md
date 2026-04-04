# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.7] - 2026-04-04

### Added
- Hub: **Pull** on each repo card, **Update all from remote** and **multi-select batch update** under the ⋯ menu (`git fetch origin` + `reset --hard FETCH_HEAD`, with stash when needed); in-app confirm dialogs instead of the browser confirm box.
- Backend: `hub_pull_from_remote_auto_many` / `hub_pull_from_remote_auto_all` plus `hub-pull-progress` events for live per-repo progress during batch updates.
- Help: **Update repositories from remote** section (`#help-remote-update`) and how it differs from **Sync All HEADs**.
- Sidebar: **Recent** list (last opened repos, up to 8).
- First-run repository root onboarding: **Skip for now** (stored separately from choosing a folder).

### Changed
- Hub: clearer batch results (including partial success: summary info banner + failure details); opening clone, scan, ZIP import, sync heads, refresh, or remote updates clears the app shell top notice to avoid stacked messages.
- Help opens to the remote-update anchor when launched from Hub links.

## [1.1.6] - 2026-04-03

### Fixed
- Hub overwrite update: when local has uncommitted tracked changes, automatically stashes and proceeds with `fetch + reset --hard` so users are not blocked by a confusing “cannot overwrite” message.

### Changed
- Conflict dialog feedback is clearer: overwrite reports whether local changes were stashed (and hints `git stash pop` to restore).

## [1.1.5] - 2026-04-03

### Changed
- Releases UI: list-only cards with separate middle modal for view/edit; clearer save feedback and more predictable cancel behavior.
- Releases actions: danger styling for delete, and “reveal in folder” support for imported assets using in-repo paths.
- Releases import: simplified to file-only selection (folder import disabled) to avoid inconsistent behavior.

## [1.1.4] - 2026-04-03

### Added
- Project intro and Hub tags persist to `.deskvio/project.json` in the repository; synced with the Hub database on save, scan, add, clone, and when opening a repo. About sidebar shows the project intro; settings explain Git vs local-only choices.
- Code tab: open the selected worktree file in the default system app or reveal it in the file manager (non-bare repos).

### Changed
- File open / reveal actions moved from a standalone row into the breadcrumb bar as compact icon buttons; removes the visual gap between breadcrumbs and the Preview / Code toggle.

## [1.1.3] - 2026-04-03

### Added
- When cloning an HTTPS URL, detect an existing Hub repo whose `origin` matches the URL; offer **clone as copy** or **update in place** via `git fetch` and `git reset --hard FETCH_HEAD`. Overwrite is blocked if the working tree is dirty.

### Changed
- Hub UI polish: clone and conflict dialogs (focus, Escape, backdrop click to dismiss), light motion on cards, search field, star button, and “more” menu; theme-aware conflict scrim.
- Pin Rust build output under `src-tauri/target` with `src-tauri/.cargo/config.toml` so a misplaced global `CARGO_TARGET_DIR` does not break local or CI builds.

## [1.1.2] - 2026-04-02

### Fixed
- Cancel clone returns deterministic cleanup status and improves partial download cleanup reliability.
- Notice banners now auto-dismiss for `success`/`info` tones to avoid persistent UX.

## [1.1.1] - 2026-04-02

### Fixed
- Respect the configured repository root directory when cloning/importing (Tauri command parameter casing).
- Make “Cancel clone” stop `git clone` immediately.
- “Refresh List” now prunes missing repositories from the Hub records.
- Fix dangerous delete failing when using custom repository roots.

## [1.0.0] - 2026-04-01

### Added
- Repo-level Releases manager (create/edit/delete release metadata).
- Multi-asset release import and local persistence in `.deskvio/releases/`.
- Backend release guards: duplicate version validation and collision-safe asset import IDs.
- Asset cleanup paths for release/asset deletion consistency.
- Unsaved-change guard for Releases editing flow.
- Third-party license manifest and project-level MIT license.

### Changed
- Reframed product scope to offline repository backup release management (non-CI publishing workflow).
- Removed bundled sample repository content from source tree to reduce redistribution risk.

### Fixed
- Prevented release asset overwrite risk on import collisions.
- Improved release metadata write safety and consistency tests.
