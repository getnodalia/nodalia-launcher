<p align="center">
  <a href="https://getnodalia.com">
    <img src="https://raw.githubusercontent.com/getnodalia/nodalia-web/main/assets/brand/nodalia-wordmark-badge.svg" width="150" alt="Nodalia">
  </a>
</p>

<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/dev.danielmiguelt.NodaliaLauncher.png" width="128" height="128" alt="Nodalia Launcher icon">
</p>

<h1 align="center">Nodalia Launcher</h1>

<p align="center">
  <b>One place to install, update and open every Nodalia app.</b><br>
  Letra, Celdas, Lámina, Papel, Buzón, Hito, Ágora, Lienzo, Pluma, Pliego, Luz, Toma, Pulso, Ánima, Trama, Forja, Torno, Vortia, Vinilo and Brújula: the right build for your computer, kept up to date.
</p>

<p align="center">
  <img alt="100% Rust" src="https://img.shields.io/badge/100%25-Rust-b7410e?style=flat-square&logo=rust">
  <img alt="macOS · Windows · Linux · FreeBSD" src="https://img.shields.io/badge/macOS%20%C2%B7%20Windows%20%C2%B7%20Linux%20%C2%B7%20FreeBSD-native-26282e?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-3a3a3a?style=flat-square">
  <img alt="Status: early" src="https://img.shields.io/badge/status-early-d69e2e?style=flat-square">
</p>

<p align="center">
  <a href="https://github.com/getnodalia/nodalia-launcher/releases/latest"><b>Download Nodalia Launcher</b></a> ·
  <a href="https://getnodalia.com/">getnodalia.com</a>
</p>

## What it does

Every Nodalia app ships its own builds for every platform. Nodalia Launcher saves you from hunting
through two dozen release pages: it lists every app, shows which ones you have and at which version,
installs the right build for your operating system and processor, and tells you when an update is
out. It is a small native app, drawn in the Nodalia Design Language, that can sit in your menu bar
or system tray.

- **Install in one click.** It picks the right download for your system (DMG, MSI, AppImage, .deb,
  .rpm and more), checks it against the release's SHA-256 checksums and installs it.
- **Stay up to date.** It checks each app's GitHub releases on a schedule, tells you with a
  notification, and can install updates by itself. **Settings → Updates** turns automatic updates
  on or off and sets how often to check.
- **Updates itself.** Nodalia Launcher replaces its own copy through the same flow (AppImage, macOS
  app, Windows MSI).
- **Always at hand.** Open any app from one window, or from the tray and menu-bar icon. On Linux
  its menu also shows in the desktop's top bar over D-Bus.
- **Native.** egui on wgpu. No Electron, no web view.

Every download comes only from Nodalia's release pages on GitHub and is verified against its
published SHA-256 before it is installed. A running app is never replaced.

## Interface languages

English, Spanish (Español), German (Deutsch), French (Français), Brazilian Portuguese (Português
(Brasil)), Polish (Polski), Czech (Čeština), Hungarian (Magyar), Russian (Русский), Ukrainian
(Українська), Georgian (ქართული), Japanese (日本語), Simplified Chinese (简体中文) and Traditional
Chinese (繁體中文). **Auto**, the default, follows the system language; **Settings → Language**
picks one and switches at once. To try a language without changing the setting, start the app with
`NODALIA_LAUNCHER_LOCALE=es`. App names stay as published.

The CJK languages and Georgian are drawn with the system's own font; where the system has none,
the interface stays in English and Settings says why. See
[`docs/localization-zh-hant.md`](docs/localization-zh-hant.md) for the Traditional Chinese details.

## Where apps are installed

Nodalia Launcher installs each app where your system normally keeps applications, using the app's
official release package, so an app installed by the launcher is the same as one you installed by
hand (and the launcher finds apps you installed yourself):

| System | Where | How |
|---|---|---|
| macOS | `/Applications/<App>.app` | Copied from the release DMG |
| Windows | `%LOCALAPPDATA%\Programs\Nodalia\<App>` (or `Program Files\Nodalia\<App>` for everyone on the computer) | The release MSI, through Windows Installer |
| Linux | `/usr/bin`, `/usr/share/applications` | The release `.deb` (apt) or `.rpm` (dnf, zypper), with your system's password prompt; the AppImage or tarball where those aren't an option |
| FreeBSD | `/usr/local` | The release tree, with a password prompt (polkit); `~/.local` without polkit |

Updates use the same format an app was installed with. The launcher writes its log to
`logs/nodalia-launcher.log` in its data folder (`~/.local/share/nodalia-launcher` on Linux,
`~/Library/Application Support/Nodalia Launcher` on macOS, `%LOCALAPPDATA%\Nodalia Launcher` on
Windows) and keeps the previous two runs. `RUST_LOG` changes what it records.

## Downloads

Get the build for your computer from the
[latest release](https://github.com/getnodalia/nodalia-launcher/releases/latest).

| System | Files |
|---|---|
| Windows 10/11 | `nodalia-launcher-<ver>-windows-{x64,arm64,x86}.msi` (per-user, no admin rights needed) and `…-windows-{x64,arm64,x86}-portable.zip` |
| macOS 11+ | `nodalia-launcher-<ver>-macos-universal.dmg` (Apple silicon and Intel; unsigned, so the first open needs a right click → Open) |
| Linux x86_64 | `nodalia-launcher-<ver>-linux-x86_64.{AppImage,deb,rpm,tar.gz}` |

Checksums are in `SHA256SUMS`, `SHA256SUMS-macos` and `SHA256SUMS-windows`. The tray icon uses the
StatusNotifierItem standard: KDE, Xfce, Cinnamon and most other desktops show it; on GNOME install
the AppIndicator extension.

## Build from source

You need a recent stable Rust. On Linux, install the windowing headers first (Debian/Ubuntu:
`sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxrandr-dev libxi-dev libgl1-mesa-dev`).

```sh
git clone https://github.com/getnodalia/nodalia-launcher
cd nodalia-launcher
cargo run --release -p nodalia-launcher   # the desktop app
cargo xtask ci                             # fmt, clippy, tests and the wasm check
```

`flake.nix` gives a Nix dev shell (`nix develop`) and a package (`nix run github:getnodalia/nodalia-launcher`).
On NixOS it uses the system's graphics drivers; elsewhere run it through
[nixGL](https://github.com/nix-community/nixGL), or use the AppImage, `.deb` or `.rpm`.

New contributors and AI agents: start with [`AGENTS.md`](AGENTS.md). Release packaging lives in
[`packaging/`](packaging/) and [`.github/workflows/`](.github/workflows/).

## Releases

A pull request that bumps `version` in `Cargo.toml` and adds `docs/release-notes/vX.Y.Z.md` releases
it when it merges: the tag, the packages and the release are all automatic. See
[`docs/release-notes/README.md`](docs/release-notes/README.md).

## The Nodalia suite

Nodalia Launcher is the application that installs and updates the others in [Nodalia](https://getnodalia.com),
an independent software project developed in public by
[Daniel Miguel Tejedor](https://danielmigueltejedor.com).

## License and credits

Nodalia Launcher is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. The Nodalia name, mark and app icons are Nodalia's. Third-party material (fonts, icons,
dependencies) and its licences are listed in [`ATTRIBUTION.md`](ATTRIBUTION.md) and
[`NOTICE`](NOTICE).
