//! Native file dialogs, through the desktop's own file chooser (the
//! xdg-desktop-portal FileChooser), so they match the rest of the system.

use std::path::{Path, PathBuf};

/// Ask the user to choose a folder, starting at `start`. `None` if cancelled.
pub fn pick_folder(start: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("Choose a folder");
    if start.is_dir() {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_folder()
}

/// Whether `pick_folder` can do anything on this platform.
pub const fn available() -> bool {
    true
}
