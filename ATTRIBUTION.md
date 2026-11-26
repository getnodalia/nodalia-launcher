# Attribution

Every non-code asset in this repository, with its author, source and license. Nodalia Launcher's own
code and original assets are MIT OR Apache-2.0 (see [`LICENSE-MIT`](LICENSE-MIT),
[`LICENSE-APACHE`](LICENSE-APACHE) and [`NOTICE`](NOTICE)). When you add an asset, add a row here
in the same change; third-party assets must be under a license on the allowlist and keep their
license file next to them.

## Bundled in the app

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `assets/brand/` (all files) | The Nodalia mark and wordmarks, and their tray / menu-bar renders | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/logo/`, identical to `nodalia-web`'s `assets/`; the PNG renders are made from the SVGs unmodified | MIT OR Apache-2.0 |
| `assets/app-icon/` (all files) | Nodalia Launcher app icon: the Nodalia mark on its dark tile | The Nodalia contributors | Rendered from `assets/brand/nodalia-mark.svg` by `packaging/icons.sh` and `cargo xtask ico` / `cargo xtask icns` | MIT OR Apache-2.0 |
| `packaging/macos/dmg/` (all files) | macOS DMG window background and layout | The Nodalia contributors | Original work, see [`packaging/macos/dmg/README.md`](packaging/macos/dmg/README.md); drawn with `draw_background.py` in nodalia-design's DMG-window layout | MIT OR Apache-2.0 |
| `assets/fonts/Inter-Variable.ttf` | Inter 4.001, variable (body text) | The Inter Project Authors (Rasmus Andersson) | Google Fonts, <https://github.com/google/fonts/tree/main/ofl/inter> (`Inter[opsz,wght].ttf`, renamed) | OFL-1.1, [`assets/fonts/OFL-Inter.txt`](assets/fonts/OFL-Inter.txt) |
| `assets/fonts/Sora-Variable.ttf` | Sora, variable (titles and app names) | The Sora Project Authors | [sora-xor/sora-font](https://github.com/sora-xor/sora-font), via nodalia-design `packages/fonts` | OFL-1.1, [`assets/fonts/OFL-Sora.txt`](assets/fonts/OFL-Sora.txt) |
| `crates/core/tests/fixtures/*-release.json` | Trimmed GitHub release responses used as parser fixtures | The Nodalia contributors | Real release metadata of Nodalia apps, reduced to the fields the parser reads | MIT OR Apache-2.0 |
| (dependency) `epaint_default_fonts/fonts/NotoEmoji-Regular.ttf` | Noto Emoji Regular, monochrome emoji fallback for arbitrary release-note, feed and alert text | The Noto Project Authors (Google) | Bundled transitively by `egui 0.37`'s `monochrome_emoji_fonts` feature from `epaint_default_fonts`; <https://github.com/getnodalia/egui/tree/6d518fc799681571963842f95b55a6b68431b15a/crates/epaint_default_fonts/fonts> | SIL OFL 1.1, `epaint_default_fonts/fonts/OFL.txt`; see also the dependency `README` and egui `README.md` |
| (dependency) `epaint_default_fonts/fonts/emoji-icon-font.ttf` | emoji-icon-font, monochrome symbol/emoji fallback | John Slegers | Bundled transitively by `egui 0.37`'s `monochrome_emoji_fonts` feature from `epaint_default_fonts`; <https://github.com/jslegers/emoji-icon-font> | MIT, Copyright (c) 2014 John Slegers, `epaint_default_fonts/fonts/emoji-icon-font-mit-license.txt` |
| (dependency) `epaint_default_fonts/fonts/egui-icons.ttf` | egui-icons, five-glyph subset of emoji-icon-font | John Slegers | [egui 6d518fc7](https://github.com/getnodalia/egui/tree/6d518fc799681571963842f95b55a6b68431b15a/crates/epaint_default_fonts/fonts); included by default_fonts independently of monochrome_emoji_fonts | MIT, Copyright (c) 2014 John Slegers; full upstream text in `NOTICE` |
| (dependency) `epaint_default_fonts/fonts/Ubuntu-Light.ttf` | Ubuntu Light, default font fallback | Canonical Ltd. | [Pinned egui source](https://github.com/getnodalia/egui/tree/6d518fc799681571963842f95b55a6b68431b15a/crates/epaint_default_fonts/fonts) | Ubuntu Font Licence 1.0; full upstream text and copyright in `NOTICE` |
| (dependency) `epaint_default_fonts/fonts/Hack-Regular.ttf` | Hack Regular, default monospace fallback | Source Foundry Authors; Bitstream Inc.; DejaVu contributors | [Pinned egui source](https://github.com/getnodalia/egui/tree/6d518fc799681571963842f95b55a6b68431b15a/crates/epaint_default_fonts/fonts) | MIT and Bitstream Vera License; DejaVu work public domain; full upstream notices in `NOTICE` |

## Nodalia app icons

The launcher shows each app with that app's own icon. They are copied from the Nodalia brand set
(which collects each app's repository), downscaled to 128 px. Refresh them from there, never
redraw them.

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `assets/app-icons/letra.png` | Letra app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/celdas.png` | Celdas app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/lamina.png` | Lamina app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/papel.png` | Papel app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/buzon.png` | Buzon app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/hito.png` | Hito app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/agora.png` | Agora app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/lienzo.png` | Lienzo app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/pluma.png` | Pluma app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/pliego.png` | Pliego app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/luz.png` | Luz app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/toma.png` | Toma app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/pulso.png` | Pulso app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/anima.png` | Anima app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/trama.png` | Trama app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/forja.png` | Forja app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/torno.png` | Torno app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/vortia.png` | Vortia app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/vinilo.png` | Vinilo app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/brujula.png` | Brujula app icon, 128 px | The Nodalia contributors | [`getnodalia/foundation`](https://github.com/getnodalia/foundation) `brand/app-icons/` (each app's repository is the source), downscaled | MIT OR Apache-2.0 |
| `assets/app-icons/codigo.png` | Codigo app icon, 128 px | The Nodalia contributors | [`getnodalia/codigo`](https://github.com/getnodalia/codigo) `assets/icons/hicolor/128x128/apps/dev.danielmiguelt.Codigo.png` (the app's icon: a public-domain portrait of Grace Hopper), unchanged | MIT OR Apache-2.0 |
| `assets/app-icons/orbe.png` | Orbe app icon, 128 px | The Nodalia contributors | [`getnodalia/orbe`](https://github.com/getnodalia/orbe) `assets/icons/hicolor/128x128/apps/dev.danielmiguelt.Orbe.png` (the app's icon: a public-domain portrait of Johannes Kepler), unchanged | MIT OR Apache-2.0 |
| `assets/app-icons/circuito.png` | Circuito app icon, 128 px | The Nodalia contributors | [`getnodalia/circuito`](https://github.com/getnodalia/circuito) `assets/icons/hicolor/128x128/apps/dev.danielmiguelt.Circuito.png` (the app's icon: a public-domain portrait of Gustav Kirchhoff), unchanged | MIT OR Apache-2.0 |
| `assets/app-icons/abaco.png` | Abaco app icon, 128 px | The Nodalia contributors | [`getnodalia/abaco`](https://github.com/getnodalia/abaco) `assets/icons/hicolor/128x128/apps/dev.danielmiguelt.Abaco.png` (the app's icon: a public-domain portrait of John Napier), unchanged | MIT OR Apache-2.0 |
