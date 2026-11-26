//! Modal dialogs: release notes, uninstall confirmation, settings.

use crate::i18n::fmt;
use egui::{RichText, vec2};
use nodalia_launcher_core::Os;
use nodalia_launcher_core::protocol::Command;
use nodalia_theme::about::{self, Info, Link};

use crate::app::{Dialog, Host, LauncherUi};
use crate::theme::{self, Tokens};
use crate::widgets::{self, Kind};

impl LauncherUi {
    /// Draws the current modal and writes any edits made in it back to `self.dialog`. Each dialog kind gets its own modal id
    /// so a smaller prompt cannot inherit a taller one's last size, and closing (button, Escape or
    /// a click outside) discards the dialog.
    pub(crate) fn dialogs(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        let Some(mut dialog) = self.dialog.clone() else {
            return;
        };
        let t = Tokens::get(ctx);
        // An area is laid out in the size it had last frame, so dialogs can't share one id: the
        // uninstall prompt would open as tall as Settings and never shrink (#88).
        // The large dialogs run their header to the card's edges; the small prompts keep a margin.
        let large = matches!(dialog, Dialog::Settings | Dialog::Notes(_));
        let modal_id = egui::Id::unique("dialog").with(dialog.key());
        // Below the macOS traffic lights: the dialog is centred in the window less their strip.
        let area = egui::Modal::default_area(modal_id).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, self.top_inset / 2.0));
        let modal = egui::Modal::new(modal_id).area(area).frame(if large { crate::sheet::frame(&t) } else { crate::sheet::small_frame(&t) });
        let mut close = false;
        let response = match &mut dialog {
            Dialog::Notes(id) => modal.show(ctx, |ui| close = self.notes_dialog(ui, id)),
            Dialog::ConfirmUninstall(id) => modal.show(ctx, |ui| close = self.uninstall_dialog(ui, id)),
            Dialog::UnlockVersion { app, tag } => modal.show(ctx, |ui| close = self.unlock_version_dialog(ui, app, tag)),
            Dialog::Install { app, folder } => modal.show(ctx, |ui| close = self.install_dialog(ui, app, folder, host)),
            Dialog::Replace { app, path } => modal.show(ctx, |ui| close = self.replace_dialog(ui, app, path)),
            Dialog::Settings => modal.show(ctx, |ui| close = self.settings_dialog(ui, host)),
            Dialog::LauncherUpdate(r) => modal.show(ctx, |ui| close = self.launcher_update_dialog(ui, r)),
            Dialog::Versions(id) => modal.show(ctx, |ui| close = self.version_dialog(ui, id)),
            Dialog::LaunchFlags { app, flags } => modal.show(ctx, |ui| close = self.launch_flags_dialog(ui, app, flags)),
            Dialog::About => modal.show(ctx, |ui| close = self.about_dialog(ui)),
            Dialog::Shortcuts => modal.show(ctx, |ui| close = self.shortcuts_dialog(ui)),
        };
        if self.dialog.as_ref().is_some_and(|active| active.key() != dialog.key()) {
            // A control inside a dialog opened a different one; don't restore the old dialog.
            return;
        }
        if close || response.should_close() {
            self.dialog = if matches!(&dialog, Dialog::UnlockVersion { app, .. } if app == nodalia_launcher_core::catalog::LAUNCHER_ID) {
                Some(Dialog::Settings)
            } else {
                None
            };
            if matches!(dialog, Dialog::Versions(_)) {
                // Close, Escape or a click outside: a version chosen there is dropped.
                self.revert_version_choice();
            }
        } else {
            self.dialog = Some(dialog);
        }
    }

    /// NDL's About window: the mark, the name and version, what the launcher is, the Nodalia
    /// suites, the links and the licence line. It has no Contributors or Models tabs: credits live
    /// in `NOTICE`.
    fn about_dialog(&mut self, ui: &mut egui::Ui) -> bool {
        use nodalia_launcher_core::catalog::{DONATE_URL, GITHUB_URL, ISSUES_URL, WEBSITE_URL};
        let t = Tokens::get(ui.ctx());
        let (mode, accent) = (if t.dark { nodalia_theme::Mode::Dark } else { nodalia_theme::Mode::Light }, nodalia_theme::Accent::Nodalia);
        let look = nodalia_theme::creator::Look::new(mode.palette_with(accent));
        ui.set_width(about::measures::WIDTH.min(ui.ctx().content_rect().width() - 64.0));
        let close = about::heading(ui, &look, tl!("About Nodalia Launcher"), egui::Id::NULL.with("about.close"));
        let version = fmt(tl!("Version {version}"), &[("version", env!("CARGO_PKG_VERSION"))]);
        let links = [
            Link { label: tl!("Website"), url: WEBSITE_URL },
            Link { label: tl!("GitHub"), url: GITHUB_URL },
            Link { label: tl!("Report an issue"), url: ISSUES_URL },
            Link { label: tl!("Support Nodalia"), url: DONATE_URL },
        ];
        let info = Info {
            name: "Nodalia Launcher",
            version: &version,
            tagline: tl!("Install, update and open the Nodalia apps from one place."),
            suite: tl!("Office · Creator · Engineering · Enterprise"),
            links: &links,
            footer: "MIT OR Apache-2.0",
        };
        let page = about::page(ui, &look, &info, |ui, rect| {
            egui::Image::new(crate::icons::nodalia_mark()).fit_to_exact_size(rect.size()).paint_at(ui, rect);
        });
        if let Some(link) = page.picked.and_then(|i| links.get(i)) {
            widgets::open_external_link(ui.ctx(), link.url);
        }
        close.closed
    }

    /// The keyboard shortcuts the launcher answers to.
    fn shortcuts_dialog(&mut self, ui: &mut egui::Ui) -> bool {
        let t = Tokens::get(ui.ctx());
        ui.set_width(420.0f32.min(ui.ctx().content_rect().width() - 80.0));
        Self::title(ui, None, tl!("Keyboard shortcuts"), "");
        let mut rows = vec![
            (tl!("Settings"), crate::platform_label("Ctrl+,", "⌘,")),
            (tl!("Check for updates"), crate::platform_label("Ctrl+R", "⌘R")),
            (tl!("Keyboard shortcuts"), crate::platform_label("Ctrl+/", "⌘/")),
            (tl!("Zoom in the release notes"), crate::platform_label("Ctrl++", "⌘+")),
            (tl!("Zoom out the release notes"), crate::platform_label("Ctrl+-", "⌘-")),
            (tl!("Close this window"), "Esc".to_string()),
        ];
        if self.caps.manage_apps {
            rows.push((tl!("Quit Nodalia Launcher"), crate::platform_label("Ctrl+Q", "⌘Q")));
        }
        for (label, keys) in rows {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(keys).family(egui::FontFamily::Monospace).color(t.muted));
                });
            });
            ui.add_space(2.0);
        }
        ui.add_space(12.0);
        let mut close = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = widgets::button(ui, tl!("Close"), Kind::Primary, 90.0).clicked();
        });
        close
    }

    /// Draws the optional icon beside a heading and an optional subtitle. The icon is decorative so
    /// assistive technology hears the title once, and an empty subtitle is omitted.
    fn title(ui: &mut egui::Ui, icon: Option<egui::ImageSource<'static>>, title: &str, subtitle: &str) {
        let t = Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            if let Some(icon) = icon {
                // The heading beside it carries the title; an icon with the same accessible
                // name would announce the dialog title twice.
                ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(40.0, 40.0)));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(widgets::heading(title, theme::DIALOG_TITLE, t.ink_strong));
                if !subtitle.is_empty() {
                    widgets::caption(ui, subtitle, t.faint);
                }
            });
        });
        ui.add_space(14.0);
    }

    /// An app's release notes: a large card like the app page, the notes in a scrolling body.
    fn notes_dialog(&mut self, ui: &mut egui::Ui, id: &str) -> bool {
        let Some(entry) = self.entry(id).cloned() else { return true };
        let Some(release) = entry.release().cloned() else { return true };
        let t = Tokens::get(ui.ctx());
        let size = crate::sheet::size(ui.ctx(), vec2(880.0, 760.0) * self.notes_zoom.max(1.0), self.top_inset);
        ui.set_width(size.x);
        ui.set_max_height(size.y);
        let date = release.published_at.map(|d| fmt(tl!("Released {date}"), &[("date", &crate::views::date_text(d))])).unwrap_or_default();
        let hero_h = crate::sheet::hero(ui, crate::icons::app(id), &format!("{} {}", entry.app.name, release.version_label()), &date, |_| {});
        let footer_h = theme::CONTROL_H + 32.0;
        let body_h = (size.y - hero_h - footer_h - 2.0).max(0.0);
        let saved = crate::sheet::accent_scroll(ui, &t, t.accent);
        egui::ScrollArea::vertical().id_salt("notes-body").max_height(body_h).auto_shrink([false, true]).show(ui, |ui| {
            *ui.visuals_mut() = saved;
            egui::Frame::new().inner_margin(egui::Margin::same(theme::PAD as i8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if release.notes.is_empty() {
                    ui.label(tl!("These notes weren't fetched (GitHub's request limit was reached). They're on the release page."));
                } else {
                    crate::notes::show_scaled(ui, &release.notes, self.notes_zoom);
                }
            });
        });
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), t.hairline());
        let mut close = false;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(theme::PAD as i8, 16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                widgets::caption_link(ui, tl!("Open on GitHub"), &release.html_url);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = widgets::button(ui, tl!("Close"), Kind::Secondary, 90.0).clicked();
                });
            });
        });
        close
    }

    /// Offers a new launcher version and returns whether the dialog should close. With self-update
    /// it queues `UpdateLauncher` and closes; otherwise it opens the platform download (or the
    /// release page) and closes. "Later" closes without doing either.
    fn launcher_update_dialog(&mut self, ui: &mut egui::Ui, r: &nodalia_launcher_core::Release) -> bool {
        let t = Tokens::get(ui.ctx());
        ui.set_width(480.0f32.min(ui.ctx().content_rect().width() - 80.0));
        let title = fmt(tl!("Nodalia Launcher {version} is available."), &[("version", &r.version_label())]);
        Self::title(ui, Some(crate::icons::nodalia_mark()), &title, "");
        let message = if self.caps.self_update {
            tl!("A new version is available. Would you like to relaunch with the updated version?")
        } else {
            tl!("A new version is available. Download it to replace this version?")
        };
        ui.label(RichText::new(message).color(t.muted));
        ui.add_space(14.0);
        let mut close = false;
        ui.horizontal(|ui| {
            widgets::caption_link(ui, tl!("What's new"), &r.html_url);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.caps.self_update {
                    if widgets::button(ui, tl!("Update and restart"), Kind::Primary, 0.0)
                        .on_hover_text(tl!("Downloads the new version, replaces this one and opens it again"))
                        .clicked()
                    {
                        self.act_command(Command::UpdateLauncher);
                        close = true;
                    }
                } else {
                    let url = self
                        .platform
                        .and_then(|p| crate::views::launcher_download(r, p, crate::settings::is_flatpak_sandbox()))
                        .unwrap_or_else(|| r.html_url.clone());
                    if widgets::button(ui, tl!("Download"), Kind::Primary, 0.0).clicked() {
                        widgets::open_external_link(ui.ctx(), &url);
                        close = true;
                    }
                }
                if widgets::button(ui, tl!("Later"), Kind::Secondary, 80.0).clicked() {
                    close = true;
                }
            });
        });
        close
    }

    /// Confirms removing an installed app and returns `true` when the dialog should close, including
    /// when the app entry has disappeared. It queues `Uninstall` only on confirmation and explains
    /// the removal differently for each install source.
    fn uninstall_dialog(&mut self, ui: &mut egui::Ui, id: &str) -> bool {
        let Some(entry) = self.entry(id).cloned() else { return true };
        let t = Tokens::get(ui.ctx());
        ui.set_width(420.0);
        Self::title(ui, crate::icons::app(id), &fmt(tl!("Uninstall {name}?"), &[("name", entry.app.name)]), "");
        let how = match entry.installed.as_ref().map(|i| i.source) {
            Some(nodalia_launcher_core::InstallSource::MacApp) => tl!("The app will be moved to the Trash. Your documents and settings stay."),
            Some(nodalia_launcher_core::InstallSource::Msi) => {
                tl!("Windows will remove it like any installed program. Your documents and settings stay.")
            }
            Some(
                nodalia_launcher_core::InstallSource::Deb | nodalia_launcher_core::InstallSource::Rpm | nodalia_launcher_core::InstallSource::Pacman,
            ) => tl!("Your system's package manager will remove it, and may ask for your password. Your documents and settings stay."),
            _ => tl!("The app will be removed. Your documents and settings stay."),
        };
        ui.label(RichText::new(how).color(t.muted));
        ui.add_space(20.0);
        let mut close = false;
        // A row: a right-to-left layout straight in the column would fill the height left.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, tl!("Uninstall"), Kind::Danger, 110.0).clicked() {
                    self.act_command(Command::Uninstall { app: id.to_string() });
                    close = true;
                }
                if widgets::button(ui, tl!("Cancel"), Kind::Secondary, 90.0).clicked() {
                    close = true;
                }
            });
        });
        close
    }

    /// The extra command-line flags an app is opened with. Saved only when they split cleanly.
    fn launch_flags_dialog(&mut self, ui: &mut egui::Ui, id: &str, flags: &mut String) -> bool {
        let Some(app) = self.entry(id).map(|e| e.app) else { return true };
        let t = Tokens::get(ui.ctx());
        ui.set_width(460.0f32.min(ui.ctx().content_rect().width() - 80.0));
        Self::title(ui, crate::icons::app(id), &fmt(tl!("{name} launch options"), &[("name", app.name)]), "");
        let label = ui.label(tl!("Flags added each time the app is opened"));
        ui.add(egui::TextEdit::singleline(flags).id_salt("launch_flags").hint_text("--flag \"…\"").desired_width(f32::INFINITY))
            .labelled_by(label.id);
        let split = nodalia_launcher_core::args::split(flags);
        let (note, color) = match split {
            Ok(_) => (tl!("Separate flags with spaces. Quote a value that has spaces in it."), t.muted),
            Err(_) => (tl!("A quote isn't closed."), t.bad),
        };
        ui.label(RichText::new(note).color(color));
        ui.add_space(14.0);
        let mut close = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save = ui.add_enabled_ui(split.is_ok(), |ui| widgets::button(ui, tl!("Save"), Kind::Primary, 90.0)).inner;
                if save.clicked() {
                    let trimmed = flags.trim();
                    if trimmed.is_empty() {
                        self.settings.launch_flags.remove(id);
                    } else {
                        self.settings.launch_flags.insert(id.to_string(), trimmed.to_string());
                    }
                    close = true;
                }
                close |= widgets::button(ui, tl!("Cancel"), Kind::Secondary, 90.0).clicked();
            });
        });
        close
    }

    /// Warns that `path` already exists and asks whether to replace it, returning `true` when the
    /// dialog should close, including when the app entry is gone. `Replace` is queued at the action
    /// boundary, not sent inline.
    fn replace_dialog(&mut self, ui: &mut egui::Ui, id: &str, path: &str) -> bool {
        let Some(app) = self.entry(id).map(|e| e.app) else { return true };
        let t = Tokens::get(ui.ctx());
        ui.set_width(440.0f32.min(ui.ctx().content_rect().width() - 80.0));
        Self::title(ui, crate::icons::app(id), tl!("Replace what's there?"), "");
        ui.label(
            RichText::new(fmt(tl!("{path} already exists. Installing {name} will overwrite it."), &[("path", path), ("name", app.name)]))
                .color(t.muted),
        );
        ui.add_space(20.0);
        let mut close = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, tl!("Replace"), Kind::Danger, 110.0).clicked() {
                    self.act_command(Command::Replace { app: id.to_string(), path: path.to_string() });
                    close = true;
                }
                if widgets::button(ui, tl!("Cancel"), Kind::Secondary, 90.0).clicked() {
                    close = true;
                }
            });
        });
        close
    }

    /// Lets the user choose this app's install folder and returns `true` when the dialog should
    /// close, including when the app entry is gone. An empty field removes the per-app override, a
    /// non-empty one persists it, and any changed configuration is sent before the install is queued. The path
    /// itself is not validated here.
    fn install_dialog(&mut self, ui: &mut egui::Ui, id: &str, folder: &mut String, host: &mut dyn Host) -> bool {
        let Some(app) = self.entry(id).map(|e| e.app) else { return true };
        ui.set_width(440.0f32.min(ui.ctx().content_rect().width() - 80.0));
        Self::title(ui, None, app.name, tl!("Install folder"));
        let default = self.settings.install_locations.root.clone().or_else(|| self.default_folder_for(id, host));
        Self::folder_field(ui, folder, default.as_deref(), host);
        if self.platform.is_some_and(|p| p.os == Os::Linux) {
            ui.label(tl!("Custom folders use AppImage on Linux."));
        }
        ui.add_space(14.0);
        let mut close = false;
        ui.horizontal(|ui| {
            if widgets::button(ui, tl!("Install"), Kind::Primary, 100.0).clicked() {
                if folder.is_empty() {
                    self.settings.install_locations.per_app.remove(id);
                } else {
                    self.settings.install_locations.per_app.insert(id.to_string(), folder.clone());
                }
                self.configure();
                self.act_command(Command::Install { app: id.to_string() });
                close = true;
            }
            close |= widgets::button(ui, tl!("Cancel"), Kind::Secondary, 90.0).clicked();
        });
        close
    }

    pub(crate) fn folder_field(ui: &mut egui::Ui, folder: &mut String, default: Option<&str>, host: &mut dyn Host) {
        let label = ui.label(tl!("Install folder"));
        ui.add(
            egui::TextEdit::singleline(folder)
                .id_salt("install_folder")
                .hint_text(default.unwrap_or(tl!("System default")))
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
        ui.label(tl!("Apps are placed inside this folder."));
        ui.horizontal(|ui| {
            if widgets::button(ui, tl!("Browse…"), Kind::Secondary, 100.0).clicked()
                && let Some(selected) = host.pick_folder(if folder.is_empty() { default } else { Some(folder.as_str()) })
            {
                *folder = selected;
            }
            if widgets::button(ui, tl!("Use default"), Kind::Secondary, 120.0).clicked() {
                folder.clear();
            }
        });
    }
}
