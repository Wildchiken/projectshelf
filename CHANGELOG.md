# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
