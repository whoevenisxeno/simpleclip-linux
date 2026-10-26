//! The small cards that slide in when a clip or screenshot is saved (or fails),
//! and when SimpleClip starts. A card is a click-through, never-focused overlay
//! (a wlr-layer-shell surface), so it can't steal focus from a game, and it
//! fades itself out. Drawn with tiny-skia and the app's own font, so it looks
//! the same on every compositor that offers layer-shell (Hyprland, sway, KDE,
//! COSMIC and others). Where there is no layer-shell (GNOME), it falls back to
//! a plain desktop notification.

use sc_core::config::{GeneralSettings, PopupPosition};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    registry_handlers,
};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_shm, wl_surface};
use wayland_client::{Connection, QueueHandle};

/// What the card is about; picks its colour and icon.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// Something was saved (green check).
    Saved,
    /// Something went wrong (red cross).
    Error,
    /// SimpleClip itself has news, such as starting up (blue, so it never looks like a save).
    Info,
}

/// What to show.
#[derive(Clone)]
pub struct Popup {
    pub kind: Kind,
    pub title: String,
    /// Detail lines under the title (kept short; long ones are ellipsised).
    pub lines: Vec<String>,
    /// One short line instead of the full card. The first detail line is shown
    /// beside the title and the rest are dropped.
    pub compact: bool,
}

impl Popup {
    pub fn simple(kind: Kind, title: &str, line: &str) -> Self {
        Self {
            kind,
            title: title.to_string(),
            lines: vec![line.to_string()],
            compact: false,
        }
    }
}

/// Where cards appear and whether they slide; set from the config.
#[derive(Clone, Copy)]
struct Placement {
    position: PopupPosition,
    animate: bool,
}

static PLACEMENT: Mutex<Placement> = Mutex::new(Placement {
    position: PopupPosition::TopRight,
    animate: true,
});

/// Follow the user's position and animation settings from now on.
pub fn configure(g: &GeneralSettings) {
    if let Ok(mut p) = PLACEMENT.lock() {
        *p = Placement {
            position: g.popup_position,
            animate: g.popup_animation,
        };
    }
}

static TX: OnceLock<Sender<Popup>> = OnceLock::new();

/// Queue a popup. Cheap and non-blocking; the first call starts the UI thread.
pub fn show(p: Popup) {
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel::<Popup>();
        std::thread::Builder::new()
            .name("sc-popup".into())
            .spawn(move || ui_thread(rx))
            .expect("popup thread");
        tx
    });
    let _ = tx.send(p);
}

const WIDTH: f32 = 392.0;
const COMPACT_WIDTH: f32 = 268.0;
const MARGIN: f32 = 18.0;
const BADGE: f32 = 28.0;
const RADIUS: f32 = 16.0;

static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

struct Fonts {
    regular: fontdue::Font,
    semibold: fontdue::Font,
}

fn fonts() -> &'static Fonts {
    static F: OnceLock<Fonts> = OnceLock::new();
    F.get_or_init(|| {
        let load = |b: &[u8]| {
            fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).expect("bundled font")
        };
        Fonts {
            regular: load(REGULAR),
            semibold: load(SEMIBOLD),
        }
    })
}

fn accent(kind: Kind) -> (u8, u8, u8) {
    match kind {
        Kind::Saved => (67, 200, 117),
        Kind::Error => (235, 87, 87),
        Kind::Info => (86, 156, 255),
    }
}

fn color(c: (u8, u8, u8)) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, 255)
}

/// Size of a card in logical pixels.
fn size_of(popup: &Popup) -> (u32, u32) {
    if popup.compact {
        (COMPACT_WIDTH as u32, 48)
    } else {
        (
            WIDTH as u32,
            (22.0 + 28.0 + 21.0 * popup.lines.len() as f32 + 12.0) as u32,
        )
    }
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

fn ease_in(p: f32) -> f32 {
    p.clamp(0.0, 1.0).powi(2)
}

// ------------------------------------------------------------- drawing --

/// Width of `text` at `px` pixels.
fn text_width(font: &fontdue::Font, px: f32, text: &str) -> f32 {
    text.chars()
        .map(|c| font.metrics(c, px).advance_width)
        .sum()
}

/// `text` cut with an ellipsis so it fits `max_w`.
fn fit(font: &fontdue::Font, px: f32, text: &str, max_w: f32) -> String {
    if text_width(font, px, text) <= max_w {
        return text.to_string();
    }
    let mut out: String = text.to_string();
    while !out.is_empty() && text_width(font, px, &format!("{out}\u{2026}")) > max_w {
        out.pop();
    }
    format!("{}\u{2026}", out.trim_end())
}

/// Draw one line of text, vertically centred in a row starting at `row_top`
/// that is `row_h` tall. Returns the width drawn.
#[allow(clippy::too_many_arguments)]
fn draw_text(
    pm: &mut Pixmap,
    font: &fontdue::Font,
    px: f32,
    x: f32,
    row_top: f32,
    row_h: f32,
    text: &str,
    rgb: (u8, u8, u8),
    max_w: f32,
) -> f32 {
    let text = fit(font, px, text, max_w);
    let lm = font
        .horizontal_line_metrics(px)
        .unwrap_or(fontdue::LineMetrics {
            ascent: px * 0.9,
            descent: -px * 0.2,
            line_gap: 0.0,
            new_line_size: px,
        });
    let baseline = (row_top + (row_h - (lm.ascent - lm.descent)) / 2.0 + lm.ascent).round();
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    let mut pen = x;
    for ch in text.chars() {
        let (m, cov) = font.rasterize(ch, px);
        let gx = (pen + m.xmin as f32).round() as i32;
        let gy = baseline as i32 - m.height as i32 - m.ymin;
        for row in 0..m.height as i32 {
            for col in 0..m.width as i32 {
                let a = cov[(row * m.width as i32 + col) as usize] as u32;
                if a == 0 {
                    continue;
                }
                let (dx, dy) = (gx + col, gy + row);
                if dx < 0 || dy < 0 || dx >= w || dy >= h {
                    continue;
                }
                let i = ((dy * w + dx) * 4) as usize;
                // Source-over onto a premultiplied destination.
                let inv = 255 - a;
                for (k, c) in [rgb.0, rgb.1, rgb.2].into_iter().enumerate() {
                    data[i + k] = ((c as u32 * a + data[i + k] as u32 * inv) / 255) as u8;
                }
                data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
            }
        }
        pen += m.advance_width;
    }
    pen - x
}

/// Render a card at `scale` into a premultiplied RGBA pixmap.
fn render(p: &Popup, scale: f32) -> Pixmap {
    let (lw, lh) = size_of(p);
    let (w, h) = ((lw as f32 * scale) as u32, (lh as f32 * scale) as u32);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).expect("pixmap");
    let s = scale;
    let f = fonts();

    // Card with a hairline border.
    let rr = |inset: f32| {
        let (x, y, ww, hh, r) = (
            inset,
            inset,
            w as f32 - 2.0 * inset,
            h as f32 - 2.0 * inset,
            (RADIUS * s - inset).max(1.0),
        );
        let mut pb = PathBuilder::new();
        pb.move_to(x + r, y);
        pb.line_to(x + ww - r, y);
        pb.quad_to(x + ww, y, x + ww, y + r);
        pb.line_to(x + ww, y + hh - r);
        pb.quad_to(x + ww, y + hh, x + ww - r, y + hh);
        pb.line_to(x + r, y + hh);
        pb.quad_to(x, y + hh, x, y + hh - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();
        pb.finish().expect("rounded rect")
    };
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color(color((27, 29, 39)));
    pm.fill_path(
        &rr(0.0),
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    paint.set_color(color((58, 62, 80)));
    pm.stroke_path(
        &rr(0.5),
        &paint,
        &Stroke {
            width: 1.0,
            ..Default::default()
        },
        Transform::identity(),
        None,
    );

    let pad = 16.0 * s;
    let badge = BADGE * s;
    let top = if p.compact {
        (h as f32 - badge) / 2.0
    } else {
        14.0 * s + (28.0 * s - badge) / 2.0
    };

    // Round badge with its icon.
    let kc = accent(p.kind);
    let mut circle = PathBuilder::new();
    circle.push_circle(pad + badge / 2.0, top + badge / 2.0, badge / 2.0);
    paint.set_color(color(kc));
    pm.fill_path(
        &circle.finish().expect("circle"),
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let u = badge / 28.0;
    let at = |x: f32, y: f32| (pad + x * u, top + y * u);
    let mut glyph = PathBuilder::new();
    match p.kind {
        Kind::Saved => {
            let (a, b, c) = (at(8.2, 14.6), at(12.3, 18.6), at(19.8, 9.6));
            glyph.move_to(a.0, a.1);
            glyph.line_to(b.0, b.1);
            glyph.line_to(c.0, c.1);
        }
        Kind::Error => {
            let (a, b, c, d) = (at(9.6, 9.6), at(18.4, 18.4), at(18.4, 9.6), at(9.6, 18.4));
            glyph.move_to(a.0, a.1);
            glyph.line_to(b.0, b.1);
            glyph.move_to(c.0, c.1);
            glyph.line_to(d.0, d.1);
        }
        Kind::Info => {
            let (a, b, c) = (at(11.4, 8.8), at(11.4, 19.2), at(19.6, 14.0));
            glyph.move_to(a.0, a.1);
            glyph.line_to(b.0, b.1);
            glyph.line_to(c.0, c.1);
            glyph.close();
        }
    }
    paint.set_color(color((18, 20, 28)));
    let path = glyph.finish().expect("glyph");
    if p.kind == Kind::Info {
        pm.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    } else {
        pm.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 2.6 * u,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }

    let title_px = 12.0 * 96.0 / 72.0 * s;
    let body_px = 10.0 * 96.0 / 72.0 * s;
    let text_x = pad + badge + 12.0 * s;
    let right = w as f32 - pad;
    if p.compact {
        // "Clip saved   12s" on one line.
        let tw = draw_text(
            &mut pm,
            &f.semibold,
            title_px,
            text_x,
            0.0,
            h as f32,
            &p.title,
            (240, 242, 248),
            right - text_x,
        );
        if let Some(detail) = p.lines.first() {
            let dx = text_x + tw + 10.0 * s;
            draw_text(
                &mut pm,
                &f.regular,
                body_px,
                dx,
                0.0,
                h as f32,
                detail,
                (150, 156, 172),
                right - dx,
            );
        }
    } else {
        let mut y = 14.0 * s;
        draw_text(
            &mut pm,
            &f.semibold,
            title_px,
            text_x,
            y,
            28.0 * s,
            &p.title,
            (240, 242, 248),
            right - text_x,
        );
        y += 30.0 * s;
        for (i, line) in p.lines.iter().enumerate() {
            // The first detail line (the file name) is brighter than the stats under it.
            let c = if i == 0 {
                (214, 218, 230)
            } else {
                (140, 146, 164)
            };
            draw_text(
                &mut pm,
                &f.regular,
                body_px,
                text_x,
                y,
                22.0 * s,
                line,
                c,
                right - text_x,
            );
            y += 21.0 * s;
        }
    }
    pm
}

// ------------------------------------------------------------ Wayland --

struct State {
    registry: RegistryState,
    output: OutputState,
    shm: Shm,
    pool: SlotPool,
    configured: bool,
    closed: bool,
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_layer!(State);
delegate_registry!(State);

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.closed = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.configured = true;
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState];
}

/// A connection to the compositor with everything a card needs.
struct Wl {
    conn: Connection,
    queue: wayland_client::EventQueue<State>,
    state: State,
    compositor: CompositorState,
    layer_shell: LayerShell,
}

impl Wl {
    fn connect() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).ok()?;
        let layer_shell = LayerShell::bind(&globals, &qh).ok()?;
        let shm = Shm::bind(&globals, &qh).ok()?;
        let pool = SlotPool::new(256 * 1024, &shm).ok()?;
        let mut state = State {
            registry: RegistryState::new(&globals),
            output: OutputState::new(&globals, &qh),
            shm,
            pool,
            configured: false,
            closed: false,
        };
        queue.roundtrip(&mut state).ok()?;
        Some(Self {
            conn,
            queue,
            state,
            compositor,
            layer_shell,
        })
    }

    /// The scale to draw at: the largest output scale, so the card is sharp
    /// wherever it lands.
    fn scale(&self) -> i32 {
        self.state
            .output
            .outputs()
            .filter_map(|o| self.state.output.info(&o))
            .map(|i| i.scale_factor)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// The first output's logical height, for centring vertically.
    fn output_height(&self) -> Option<i32> {
        self.state
            .output
            .outputs()
            .filter_map(|o| self.state.output.info(&o))
            .find_map(|i| i.logical_size.map(|s| s.1))
    }

    fn pump(&mut self) {
        let _ = self.conn.flush();
        if let Some(guard) = self.queue.prepare_read() {
            let _ = guard.read();
        }
        let _ = self.queue.dispatch_pending(&mut self.state);
    }
}

fn ui_thread(rx: Receiver<Popup>) {
    let mut wl: Option<Wl> = None;
    let mut failed = false;
    while let Ok(mut popup) = rx.recv() {
        // If more arrived while we were busy, show only the newest.
        while let Ok(newer) = rx.try_recv() {
            popup = newer;
        }
        if wl.is_none() && !failed {
            wl = Wl::connect();
            if wl.is_none() {
                tracing::warn!("no layer-shell compositor; save cards fall back to notifications");
                failed = true;
            }
        }
        match wl.as_mut() {
            Some(w) => {
                if !run_one(w, &popup, &rx) {
                    // The connection died (compositor restart); reconnect next time.
                    wl = None;
                }
            }
            None => fallback(&popup),
        }
    }
}

/// A plain desktop notification, for sessions without layer-shell.
fn fallback(p: &Popup) {
    let body = p.lines.join("\n");
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "SimpleClip", "-i", "camera-video", "-t", "4000"])
        .arg(&p.title)
        .arg(body)
        .status();
}

/// Show one card, animate it and take it down. Returns false if the
/// connection broke.
fn run_one(wl: &mut Wl, popup: &Popup, rx: &Receiver<Popup>) -> bool {
    let place = *PLACEMENT.lock().unwrap();
    let scale = wl.scale();
    let (lw, lh) = size_of(popup);
    let margin = MARGIN as i32;
    let qh = wl.queue.handle();

    let surface = wl.compositor.create_surface(&qh);
    let layer =
        wl.layer_shell
            .create_layer_surface(&qh, surface, Layer::Overlay, Some("simpleclip"), None);
    let (anchor, centered_v) = match place.position {
        PopupPosition::TopRight => (Anchor::TOP | Anchor::RIGHT, false),
        PopupPosition::TopLeft => (Anchor::TOP | Anchor::LEFT, false),
        PopupPosition::BottomRight => (Anchor::BOTTOM | Anchor::RIGHT, false),
        PopupPosition::BottomLeft => (Anchor::BOTTOM | Anchor::LEFT, false),
        PopupPosition::Center => (Anchor::TOP, true),
    };
    layer.set_anchor(anchor);
    layer.set_size(lw, lh);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_exclusive_zone(0);
    // Click-through: an empty input region.
    if let Ok(region) = Region::new(&wl.compositor) {
        layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
    }
    layer.wl_surface().set_buffer_scale(scale);

    // Slide in from the nearest side edge; the centre card rises instead.
    let travel = 44;
    let resting_top = if centered_v {
        wl.output_height()
            .map(|h| (h - lh as i32) / 2)
            .unwrap_or(margin)
    } else {
        margin
    };
    let set_margins = |layer: &LayerSurface, k: f32| {
        let off = ((1.0 - k) * travel as f32) as i32;
        let (mut t, mut r, mut b, mut l) = (0, 0, 0, 0);
        match place.position {
            PopupPosition::TopRight => (t, r) = (margin, margin - off),
            PopupPosition::TopLeft => (t, l) = (margin, margin - off),
            PopupPosition::BottomRight => (b, r) = (margin, margin - off),
            PopupPosition::BottomLeft => (b, l) = (margin, margin - off),
            PopupPosition::Center => t = resting_top + off / 2,
        }
        layer.set_margin(t, r, b, l);
    };
    set_margins(&layer, if place.animate { 0.0 } else { 1.0 });
    layer.commit();

    // Wait for the compositor to size it.
    wl.state.configured = false;
    wl.state.closed = false;
    let wait = Instant::now();
    while !wl.state.configured && wait.elapsed() < Duration::from_secs(2) {
        if wl.queue.roundtrip(&mut wl.state).is_err() {
            return false;
        }
    }
    if !wl.state.configured {
        return true;
    }

    let card = render(popup, scale as f32);
    let (pw, ph) = (card.width() as i32, card.height() as i32);

    let (fade_in, fade_out) = if place.animate {
        (Duration::from_millis(340), Duration::from_millis(280))
    } else {
        (Duration::from_millis(90), Duration::from_millis(160))
    };
    let hold = Duration::from_millis(if popup.compact {
        3000
    } else if popup.kind == Kind::Info {
        6500
    } else {
        5200
    });
    let top_alpha = 240.0 / 255.0;
    let started = Instant::now();
    let total = fade_in + hold + fade_out;
    let mut ok = true;
    loop {
        let t = started.elapsed();
        if t >= total || wl.state.closed {
            break;
        }
        if let Ok(next) = rx.try_recv() {
            // A newer card arrived while this one is up: show it next.
            if let Some(tx) = TX.get() {
                let _ = tx.send(next);
            }
            break;
        }
        // k: 0 = fully off to the side, 1 = in place.
        let (alpha, k) = if t < fade_in {
            let p = ease_out(t.as_secs_f32() / fade_in.as_secs_f32());
            (p * top_alpha, p)
        } else if t < fade_in + hold {
            (top_alpha, 1.0)
        } else {
            let p = ease_in((t - fade_in - hold).as_secs_f32() / fade_out.as_secs_f32());
            ((1.0 - p) * top_alpha, 1.0 - p)
        };
        let k = if place.animate { k } else { 1.0 };

        let Ok((buffer, canvas)) =
            wl.state
                .pool
                .create_buffer(pw, ph, pw * 4, wl_shm::Format::Argb8888)
        else {
            break;
        };
        // RGBA premultiplied -> BGRA, scaled by the fade.
        let a = (alpha * 256.0) as u32;
        for (dst, src) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(card.data().as_chunks::<4>().0)
        {
            dst[0] = ((src[2] as u32 * a) >> 8) as u8;
            dst[1] = ((src[1] as u32 * a) >> 8) as u8;
            dst[2] = ((src[0] as u32 * a) >> 8) as u8;
            dst[3] = ((src[3] as u32 * a) >> 8) as u8;
        }
        set_margins(&layer, k);
        layer.wl_surface().damage_buffer(0, 0, pw, ph);
        if buffer.attach_to(layer.wl_surface()).is_err() {
            break;
        }
        layer.commit();
        wl.pump();
        std::thread::sleep(Duration::from_millis(8));
        if wl.conn.flush().is_err() {
            ok = false;
            break;
        }
    }
    drop(layer);
    let _ = wl.queue.roundtrip(&mut wl.state);
    ok
}

// A card the user can't see is a bug worth catching without a compositor.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_renders_with_its_text() {
        let p = Popup {
            kind: Kind::Saved,
            title: "Clip saved".into(),
            lines: vec!["sc-sample.mp4".into(), "Sample game  \u{2022}  12s".into()],
            compact: false,
        };
        let pm = render(&p, 1.0);
        assert_eq!((pm.width(), pm.height()), (392, 104));
        // Something other than the flat card colour was drawn (badge and text).
        let distinct = pm
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] != 27 || px[1] != 29)
            .count();
        assert!(distinct > 500);
    }

    /// Writes sample cards for the README: `SC_CARD_DIR=docs/screenshots cargo test -p scd
    /// -- --ignored dump_cards`.
    #[test]
    #[ignore = "writes files"]
    fn dump_cards() {
        let dir = std::path::PathBuf::from(std::env::var("SC_CARD_DIR").expect("SC_CARD_DIR"));
        let full = Popup {
            kind: Kind::Saved,
            title: "Clip saved".into(),
            lines: vec![
                "sc-sample-20260101-120000.mp4".into(),
                "Sample game  \u{2022}  12s  \u{2022}  13.9 MB".into(),
                "1920x1080 @ 60 fps  \u{2022}  VA-API  \u{2022}  9.8 Mbps".into(),
                "Saved in 0.1s  \u{2022}  /sample".into(),
            ],
            compact: false,
        };
        let minimal = Popup {
            kind: Kind::Saved,
            title: "Clip saved".into(),
            lines: vec!["12s".into()],
            compact: true,
        };
        for (name, p) in [("save-popup", full), ("save-popup-minimal", minimal)] {
            let pm = render(&p, 2.0);
            pm.save_png(dir.join(format!("{name}.png")))
                .expect("write png");
        }
    }

    #[test]
    fn long_lines_are_ellipsised() {
        let f = &fonts().regular;
        let s = fit(f, 13.0, &"x".repeat(200), 100.0);
        assert!(s.ends_with('\u{2026}'));
        assert!(text_width(f, 13.0, &s) <= 100.0);
    }
}
