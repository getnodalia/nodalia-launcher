//! The launcher's menu as a desktop's global menu shows it: the same entries as the main menu in
//! the window's header, so a command reaches the launcher the same way from either.

use crate::app::{Dialog, LauncherUi, UiAction};

/// What a menu entry does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    Settings,
    Shortcuts,
    About,
    CheckNow,
    Quit,
}

/// A row of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item(String, MenuCommand),
    Separator,
}

/// The top-level menus with their entries, in the language in use. Where apps can't be managed
/// (the web build), the entries that act on them are left out.
pub fn tree(manage_apps: bool) -> Vec<(String, Vec<MenuEntry>)> {
    let mut entries = vec![
        MenuEntry::Item(tl!("Settings").to_string(), MenuCommand::Settings),
        MenuEntry::Item(tl!("Keyboard shortcuts").to_string(), MenuCommand::Shortcuts),
        MenuEntry::Item(tl!("About Nodalia Launcher").to_string(), MenuCommand::About),
    ];
    if manage_apps {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Item(tl!("Check for updates").to_string(), MenuCommand::CheckNow));
        entries.push(MenuEntry::Item(tl!("Quit Nodalia Launcher").to_string(), MenuCommand::Quit));
    }
    vec![(tl!("Nodalia Launcher").to_string(), entries)]
}

impl LauncherUi {
    /// Runs the entry chosen in a global menu. A dialog opens only when none is open, like the
    /// keyboard shortcuts that do the same.
    pub fn run_menu(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::Settings if self.dialog.is_none() => self.dialog = Some(Dialog::Settings),
            MenuCommand::Shortcuts if self.dialog.is_none() => self.dialog = Some(Dialog::Shortcuts),
            MenuCommand::About if self.dialog.is_none() => self.dialog = Some(Dialog::About),
            MenuCommand::CheckNow if self.caps.manage_apps => self.act(UiAction::CheckNow),
            MenuCommand::Quit if self.caps.manage_apps => self.quit_requested = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_has_the_launchers_menu_and_hides_app_actions_where_apps_cant_be_managed() {
        let full = tree(true);
        assert_eq!(full.len(), 1);
        assert_eq!(full[0].0, "Nodalia Launcher");
        let commands =
            |entries: &[MenuEntry]| entries.iter().filter_map(|e| if let MenuEntry::Item(_, c) = e { Some(*c) } else { None }).collect::<Vec<_>>();
        assert_eq!(
            commands(&full[0].1),
            [MenuCommand::Settings, MenuCommand::Shortcuts, MenuCommand::About, MenuCommand::CheckNow, MenuCommand::Quit]
        );
        assert_eq!(commands(&tree(false)[0].1), [MenuCommand::Settings, MenuCommand::Shortcuts, MenuCommand::About]);
    }
}
