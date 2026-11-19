//! The tray (menu bar) icon: show the window, check for updates, quit.

use nodalia_launcher_ui_egui::i18n;

use std::sync::mpsc::{Receiver, Sender, channel};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayAction {
    Toggle,
    Show,
    Check,
    Open(&'static str),
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    show: MenuId,
    check: MenuId,
    quit: MenuId,
    rx: Receiver<TrayAction>,
    tooltip: String,
    language: String,
    labels: Vec<(MenuItem, &'static str)>,
}

fn load_icon() -> Result<tray_icon::Icon, String> {
    // The Nodalia mark. macOS tints the black mark to the menu bar's colours as a template image;
    // elsewhere the mark on its tile is shown as supplied.
    let bytes: &[u8] = if cfg!(target_os = "macos") {
        include_bytes!("../../../assets/brand/nodalia-mark-black-44.png")
    } else {
        include_bytes!("../../../assets/brand/nodalia-mark-64.png")
    };
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| e.to_string())?.into_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).map_err(|e| e.to_string())
}

/// Names the icon for Linux and the BSDs. tray-icon's default id is "<pid>-<n>" and the title is
/// empty, and StatusNotifierItem hosts (KDE, GNOME's AppIndicator extension) fall back to the id
/// when there is no title, so the tray showed a number like "61965-1" instead of the app name.
/// Windows and macOS keep tray-icon's defaults: there the title is drawn next to the icon (macOS)
/// or unsupported (Windows), and the id never reaches the user.
fn identity(builder: TrayIconBuilder) -> TrayIconBuilder {
    if cfg!(any(target_os = "macos", target_os = "windows")) { builder } else { builder.with_id("nodalia-launcher").with_title("Nodalia Launcher") }
}

impl Tray {
    /// Creates the tray icon. `wake` repaints the UI so it sees the action, even when hidden.
    /// `on_quit` runs when Quit is picked, before the UI can see it: the UI may never get to.
    pub fn new(wake: impl Fn() + Send + Sync + Clone + 'static, on_quit: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let show = MenuItem::new(i18n::t("Show Nodalia Launcher"), true, None);
        let check = MenuItem::new(i18n::t("Check for Updates"), true, None);
        use nodalia_launcher_core::catalog::{GITHUB_URL, WEBSITE_URL};
        let website = MenuItem::new(i18n::t("Visit getnodalia.com"), true, None);
        let github = MenuItem::new(i18n::t("Nodalia on GitHub"), true, None);
        let quit = MenuItem::new(i18n::t("Quit Nodalia Launcher"), true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &check, &PredefinedMenuItem::separator(), &website, &github, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| e.to_string())?;
        let links = [(website.id().clone(), WEBSITE_URL), (github.id().clone(), GITHUB_URL)];
        let image = load_icon()?;
        let builder = identity(TrayIconBuilder::new()).with_menu(Box::new(menu));
        // macOS draws the icon as a template image (recoloured for light and dark menu bars).
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(image);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(image);
        let icon = builder.with_menu_on_left_click(cfg!(target_os = "macos")).with_tooltip("Nodalia Launcher").build().map_err(|e| e.to_string())?;
        let (tx, rx): (Sender<TrayAction>, Receiver<TrayAction>) = channel();
        let (show_id, check_id, quit_id) = (show.id().clone(), check.id().clone(), quit.id().clone());
        {
            let tx = tx.clone();
            let wake = wake.clone();
            let ids = (show_id.clone(), check_id.clone(), quit_id.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let action = if e.id == ids.0 {
                    TrayAction::Show
                } else if e.id == ids.1 {
                    TrayAction::Check
                } else if e.id == ids.2 {
                    TrayAction::Quit
                } else if let Some((_, url)) = links.iter().find(|(id, _)| *id == e.id) {
                    TrayAction::Open(url)
                } else {
                    return;
                };
                if matches!(action, TrayAction::Quit) {
                    on_quit();
                }
                let _ = tx.send(action);
                wake();
            }));
        }
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = tx.send(TrayAction::Toggle);
                wake();
            }
        }));
        let labels = vec![
            (show, "Show Nodalia Launcher"),
            (check, "Check for Updates"),
            (website, "Visit getnodalia.com"),
            (github, "Nodalia on GitHub"),
            (quit, "Quit Nodalia Launcher"),
        ];
        Ok(Self { icon, show: show_id, check: check_id, quit: quit_id, rx, tooltip: String::new(), language: String::new(), labels })
    }

    pub fn actions(&self) -> Vec<TrayAction> {
        self.rx.try_iter().collect()
    }

    /// Shows or hides the menu-bar icon to match the macOS window lifecycle.
    #[cfg(target_os = "macos")]
    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        self.icon.set_visible(visible).map_err(|e| e.to_string())
    }

    pub fn set_updates(&mut self, count: usize) {
        let lang = i18n::current();
        if self.language != lang.code() {
            for (item, source) in &self.labels {
                item.set_text(i18n::tr(lang, source));
            }
            self.language = lang.code().to_string();
        }
        let tip = if count == 0 {
            "Nodalia Launcher".to_string()
        } else {
            i18n::trn(lang, count as u64, "Nodalia Launcher · {n} update available", "Nodalia Launcher · {n} updates available")
        };
        if tip != self.tooltip {
            let _ = self.icon.set_tooltip(Some(&tip));
            self.tooltip = tip;
        }
    }

    #[allow(dead_code)]
    pub fn ids(&self) -> (&MenuId, &MenuId, &MenuId) {
        (&self.show, &self.check, &self.quit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_tray_has_a_stable_id_not_the_process_id() {
        let id = identity(TrayIconBuilder::new()).id().clone();
        let pid = std::process::id().to_string();
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            // Left as tray-icon picks it there: "<pid>-<n>".
            assert!(id.as_ref().starts_with(&pid), "{id:?}");
        } else {
            assert_eq!(id.as_ref(), "nodalia-launcher");
        }
    }

    #[test]
    fn the_macos_template_leaves_a_margin_like_the_systems_own_glyphs() {
        // tray-icon scales the image to the bar's 22 pt height, so a mark touching the edges of
        // its canvas fills the whole bar. 18 pt of 22 is what the system's glyphs use.
        let img = image::load_from_memory_with_format(include_bytes!("../../../assets/brand/nodalia-mark-black-44.png"), image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        let (w, h) = img.dimensions();
        assert_eq!((w, h), (44, 44));
        let opaque = |x: u32, y: u32| img.get_pixel(x, y).0[3] > 0;
        let xs: Vec<u32> = (0..w).filter(|&x| (0..h).any(|y| opaque(x, y))).collect();
        let ys: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| opaque(x, y))).collect();
        let (left, right) = (xs[0], w - 1 - xs[xs.len() - 1]);
        let (top, bottom) = (ys[0], h - 1 - ys[ys.len() - 1]);
        assert!([left, right, top, bottom].iter().all(|m| *m >= 3), "margins {left} {right} {top} {bottom}");
    }
}
