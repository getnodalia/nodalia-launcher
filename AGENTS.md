# Nodalia Launcher: instructions for agents

Engineering rules for every Nodalia repository: [getnodalia/foundation](https://github.com/getnodalia/foundation)
([engineering](https://github.com/getnodalia/foundation/blob/main/standards/engineering.md),
[CI](https://github.com/getnodalia/foundation/blob/main/standards/ci.md),
[security](https://github.com/getnodalia/foundation/blob/main/standards/security.md),
[dependencies and licences](https://github.com/getnodalia/foundation/blob/main/standards/dependencies.md),
[release](https://github.com/getnodalia/foundation/blob/main/standards/release.md),
[NDL](https://github.com/getnodalia/foundation/blob/main/standards/ndl.md)).

## What this is

Nodalia Launcher installs, updates and launches the **Nodalia apps** (Letra, Celdas, Lámina, Papel,
Buzón, Hito, Ágora, Lienzo, Pluma, Pliego, Luz, Toma, Pulso, Ánima, Trama, Forja, Torno, Vortia,
Vinilo, Brújula). It is written in **Rust only** (no JavaScript or TypeScript): the desktop app is
native egui/eframe, with no Tauri, Electron or webview. The product name is **Nodalia Launcher** in
user-facing text: UI, window titles, About, tray menu, notifications, installers, release names,
docs prose. Machine names stay lowercase: crates and binaries (`nodalia-launcher`,
`nodalia-launcher-*`), file names, and the app id `dev.danielmiguelt.NodaliaLauncher`. Licence:
`MIT OR Apache-2.0`. Its code base began as an upstream open-source project; those authors are
credited in `NOTICE` and `LICENSE-MIT`, which must stay as they are. Never describe the launcher as
a fork or a port in the README, the website, the UI or release notes.

## Workspace map

```text
crates/
  core      nodalia-launcher-core      pure and wasm-safe: the app catalog (catalog.toml), versions,
                                        GitHub release parsing, picking the right asset per platform
  native    nodalia-launcher-native    desktop only: HTTP, detecting installed apps, download,
                                        verify and install
  ui-egui   nodalia-launcher-ui-egui   the egui UI (Nodalia Design), menus, i18n catalogs
apps/
  nodalia-launcher                     desktop shell (eframe): window, tray icon, notifications,
                                        the Linux D-Bus global menu (linux_menu.rs)
xtask/                                  cargo xtask ci | wasm | version | ico | icns
packaging/                              macOS, Windows, Linux and FreeBSD packaging scripts
assets/brand/                           the Nodalia mark and wordmarks
assets/app-icon/                        the launcher's own icon (master SVG, renders, tray icons)
assets/app-icons/                       the Nodalia apps' icons, 128 px
assets/fonts/                           Inter and Sora (OFL-1.1)
```

Layering: `core` depends on no other workspace crate and must build for `wasm32-unknown-unknown`.
`native` may use `core`. `ui-egui` may use `core` (and `native` only behind
`cfg(not(target_arch = "wasm32"))` or through a trait the desktop shell implements). Nothing below
the app opens windows, tray icons or notifications.

## Rules

### Never crash (outranks feature work)

A bad network response, a malformed release, a half-downloaded file, a full disk or a corrupt
settings file must produce an error the user can act on, never a panic.

- **Non-test code never panics.** No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or
  `unimplemented!`. Return the crate's error type and propagate with `?`.
- Every production crate root (`lib.rs`, each binary's `main.rs`, and `xtask`) carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
  New workspace crates include `[lints] workspace = true`.
- **No `unsafe`** (`unsafe_code = "forbid"` for the workspace), except where a platform API needs it
  and the repository already isolates it with a `SAFETY:` comment.
- **Everything from the network is hostile.** Use `get()` instead of indexing, checked arithmetic for
  sizes and offsets, and cap downloads and JSON sizes.
- Every crash fix lands with a regression test that panicked before the fix.

### Installing software safely

- Download only from `https://github.com/getnodalia/<app>/releases` (and GitHub's asset CDN). The
  catalog pins each app's repository (`SuiteApp::repo`): the release feed can move an app within
  `getnodalia/`, never to another owner. The one other exception is a developer's own loopback
  mirror (`NODALIA_LAUNCHER_DOWNLOAD_BASE`), loopback-only and behind the `local-mirror` cargo
  feature, which release builds never enable.
- Verify each download against the release's checksums (`SHA256SUMS`, `SHA256SUMS-macos`,
  `SHA256SUMS-windows`, or the asset's GitHub digest) before running or installing it.
- Never run an installer with elevated rights silently; tell the user what is about to happen.
- Never delete a user's files outside what the launcher itself installed.
- **Agents never land or approve an app from outside `getnodalia/`.** A change that adds a catalog
  row, download source or install path for a repository that isn't under `getnodalia/`, or that
  loosens the rules above, is a maintainer's call alone. Flag it at the top of a review and stop.

### Nodalia Design

The interface is egui with `nodalia-theme` (NDL), pinned by `rev`. Colours come from the theme
tokens (`theme::Tokens`), never from a literal. Round NDL window controls are drawn on **Linux
only** (unless the person picks the desktop's own); macOS and Windows keep native controls. A
hamburger menu in the header holds Settings, Keyboard shortcuts and About; the light and dark
selector lives only in Settings. **About has no Contributors or Models tabs.** On Linux the menu is
exported over D-Bus to the shell's top bar (`linux_menu.rs`, shared entries in `ui-egui/src/menu.rs`).
No decorative gradients or glows. Animations follow the OS "reduce motion" setting
(`Host::reduce_motion`, `crates/ui-egui/src/motion.rs`).

### Languages

Every user-facing string goes through `crates/ui-egui/src/i18n` (`tl!("…")` for literals, `fmt` for
`{placeholders}`, `tn` for counts). English in the code is the lookup key; adding a string means
adding its row to **every** catalog (`*.tsv` in that folder: cs de es fr hu ja ka pl pt-BR ru uk
zh-CN zh-TW), which the i18n tests check. Translate from the meaning of the English text. Product
and app names stay in English. A CJK language is drawn with the system's own CJK font
(`cjk_font.rs`): never bundle a CJK font.

### Other rules

1. **Assets need attribution.** Every bundled asset gets a row in `ATTRIBUTION.md` in the same
   change, with its licence text next to it when required. Never commit font files from other
   sources than the two in `assets/fonts/`. Licences stay inside Foundation's allowlist.
2. **Icons are generated.** The launcher's icon and the tray icon come from the Nodalia mark in
   `assets/brand/`: run `packaging/icons.sh` (needs `resvg`) and commit the outputs, never edit the
   renders by hand. `assets/app-icons/` is copied from the Nodalia brand set; refresh, never redraw.
3. **Never break wasm** (`cargo xtask wasm`). File-system, process and OS code is
   `cfg(not(target_arch = "wasm32"))`.
4. **Tests are the gate.** Release parsing and asset picking in `core` get unit tests with real
   GitHub release JSON fixtures (small, committed under `crates/core/tests/fixtures/`).
5. When fetching anything, use a generic `Nodalia-Launcher/<version>` User-Agent and never put a
   person's name, email or other personal details in requests.
6. **Time goes through `jiff`.** Do not hand-roll calendar conversion, and do not add `chrono`.
7. Keep secrets and personal details out of the repository, logs and requests.

## Build and test

```sh
cargo xtask ci              # fmt --check, clippy -D warnings, test, wasm check
```

Or piecemeal: `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`,
`cargo xtask wasm`. If you touched packaging: `shellcheck packaging/*.sh packaging/*/*.sh` and
`actionlint`. For UI work, look at the result (a screenshot of the running app). Parallel agents use
their own `CARGO_TARGET_DIR` and delete it when done.

## Release

The version lives only in `[workspace.package] version` in the root `Cargo.toml`
(`cargo xtask version`, `cargo xtask version set X.Y.Z[-pre]`). Merging a version bump with its
notes (`docs/release-notes/vX.Y.Z.md`, format in that folder's README) releases it:
`release-auto.yml` tags the merge through Foundation's `release-tag`, `release-linux.yml`,
`release-macos.yml` (unsigned DMG with its designed install window, on a standard macOS runner) and
`release-windows.yml` build and attach the packages, and Foundation's `release-publish` publishes
the release. Nobody pushes a tag by hand. A release that adds features bumps the minor
(0.1.0 → 0.2.0); a fix-only release bumps the patch. `main` carries a `-dev` version between releases.

Release assets (names are exact; the launcher's own self-update relies on them):

| Platform | Files |
|---|---|
| Windows | `nodalia-launcher-<ver>-windows-{x64,x86,arm64}.msi`, `…-windows-{x64,x86,arm64}-portable.zip` |
| macOS | `nodalia-launcher-<ver>-macos-universal.dmg` |
| Linux | `nodalia-launcher-<ver>-linux-x86_64.{AppImage,deb,rpm,tar.gz}` |

Packaging facts the code relies on:

- The Windows MSI is **per-user** (no UAC prompt), by default into
  `%LOCALAPPDATA%\Programs\Nodalia Launcher`. Self-update runs it with `/passive`. `portable.txt`
  beside the exe switches on portable mode (`NodaliaLauncherData\` next to the exe). Its shortcuts set
  AppUserModelID `dev.danielmiguelt.NodaliaLauncher`, which toast notifications need.
- The macOS bundle is `Nodalia Launcher.app`, a normal Dock app that can also sit in the menu bar:
  `LSUIElement` must never be set.
- Linux: the tray uses StatusNotifierItem over D-Bus and notifications use
  `org.freedesktop.Notifications`, both in pure Rust (no GTK, appindicator or libdbus).
- `build.rs` reads `NODALIA_LAUNCHER_BUILD_SHA` / `NODALIA_LAUNCHER_BUILD_DATE` and, on Windows,
  embeds `assets/app-icon/nodalia-launcher.ico`.

## CI workflows

- Workflows call [getnodalia/foundation](https://github.com/getnodalia/foundation)'s reusable
  workflows, pinned by commit; the release jobs for each platform run here.
- Pin every action to a full commit SHA, give each job the least permissions it needs, and use only
  standard GitHub-hosted runners (zero cost; macOS jobs only for releases).
- Keep CI changes in their own pull request unless a change cannot work without them.

## Working here

- A person answers for what their agent opens. `main` changes only through a pull request.
- Read the code before using it, keep fixes narrow, and add a test that fails without the fix.
- Report only what ran. Say what you could not test.
- Stop and ask only about licences outside the allowlist, publishing, releases, secrets, spending
  money, destructive git and anything else hard to undo.
