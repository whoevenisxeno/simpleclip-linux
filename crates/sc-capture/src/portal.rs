use crate::{stream, Error, Result, Slot};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::PersistMode;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Where the portal's restore token lives, so the share dialog appears once
/// instead of at every launch.
pub fn token_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "SimpleClip")
        .map(|d| d.data_local_dir().join("screencast-token"))
}

fn load_token() -> Option<String> {
    let t = std::fs::read_to_string(token_path()?).ok()?;
    let t = t.trim().to_string();
    (!t.is_empty()).then_some(t)
}

fn save_token(token: &str) {
    if let Some(p) = token_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, token);
    }
}

/// Forget the remembered screen, so the next start asks again.
pub fn forget_token() {
    if let Some(p) = token_path() {
        let _ = std::fs::remove_file(p);
    }
}

/// Runs the whole capture lifetime: negotiate the portal ScreenCast session, open
/// the PipeWire remote, then hand off to the (blocking) PipeWire stream loop. The
/// portal session is held in this scope for the entire capture so the compositor
/// keeps streaming; it closes when this function returns.
pub async fn run(
    slot: Arc<Mutex<Slot>>,
    ready: crossbeam_channel::Sender<()>,
    stop: pipewire::channel::Receiver<()>,
    show_cursor: bool,
) -> Result<()> {
    let perr = |e: ashpd::Error| Error::Portal(e.to_string());
    let proxy = Screencast::new().await.map_err(perr)?;
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(perr)?;

    let cursor = if show_cursor {
        CursorMode::Embedded
    } else {
        CursorMode::Hidden
    };
    let token = load_token();
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(cursor)
                .set_sources(SourceType::Monitor | SourceType::Window)
                .set_multiple(false)
                .set_restore_token(token.as_deref())
                .set_persist_mode(PersistMode::ExplicitlyRevoked),
        )
        .await
        .map_err(perr)?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(perr)?
        .response()
        .map_err(perr)?;
    if let Some(t) = response.restore_token() {
        save_token(t);
    }
    let node_id = response
        .streams()
        .first()
        .ok_or(Error::NoFormat)?
        .pipe_wire_node_id();

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(perr)?;
    tracing::info!(node_id, "portal ScreenCast started");

    stream::run(fd, node_id, slot, ready, stop)
}
