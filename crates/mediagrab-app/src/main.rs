//! MediaGrab - GPUI front end (modern UI).
//!
//! The UI contains NO download logic. It does three things:
//!   1. collects options (link(s), folder, audio/video, ...)
//!   2. calls `Engine::start(...)`, which returns a stream of `Event`s
//!   3. applies each `Event` to its own state and re-renders
//!
//! Threading: GPUI has its own async executor and the engine runs on a tokio
//! runtime. They talk through a tokio mpsc channel, which can be awaited from
//! any executor, so no extra glue is needed.
//!
//! Extra dependency for this version: `rfd = "0.15"` (native "Browse folder" dialog).

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, relative, rgb, size, App, Application,
    Background, Bounds, Context, Div, Entity, FontWeight, Hsla, SharedString, Stateful,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, Root,
};
use mediagrab_core::{AudioFormat, CancelHandle, Engine, Event, Item, ItemState, Mode, Options};
use std::path::PathBuf;
use tokio::runtime::Handle;

// ---------------------------------------------------------------------------
// Brand colours (same gradient as the logo: violet -> cyan)
// ---------------------------------------------------------------------------
const ACCENT_A: u32 = 0x7c5cff;
const ACCENT_B: u32 = 0x22d3ee;
const WHITE: u32 = 0xffffff;
const GREEN: u32 = 0x22c55e;
const RED: u32 = 0xef4444;

fn brand() -> Background {
    linear_gradient(
        135.,
        linear_color_stop(rgb(ACCENT_A), 0.),
        linear_color_stop(rgb(ACCENT_B), 1.),
    )
}

/// Same colour with a different opacity (handy for tinted backgrounds).
fn alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}

struct Row {
    item: Item,
    state: ItemState,
    percent: f32,
    speed: Option<String>,
    eta: Option<String>,
}

struct MediaGrab {
    rt: Handle,
    url_input: Entity<InputState>,
    dir_input: Entity<InputState>,
    mode: Mode,
    audio_format: AudioFormat,
    max_height: Option<u32>,
    parallel: usize,
    save_cover_file: bool,
    rows: Vec<Row>,
    errors: Vec<String>,
    status: SharedString,
    running: bool,
    cancel: Option<CancelHandle>,
}

impl MediaGrab {
    fn new(rt: Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                "Paste YouTube / SoundCloud link(s): video, track or playlist. Separate several with spaces.",
            )
        });
        let default_dir = default_download_dir();
        let dir_input = cx.new(|cx| InputState::new(window, cx).default_value(default_dir));

        // Pressing Enter in the link box starts the download.
        cx.subscribe_in(
            &url_input,
            window,
            |this: &mut Self, _, ev: &InputEvent, _window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.start(cx);
                }
            },
        )
        .detach();

        Self {
            rt,
            url_input,
            dir_input,
            mode: Mode::Audio,
            audio_format: AudioFormat::Mp3,
            max_height: Some(1080),
            parallel: 4,
            save_cover_file: false,
            rows: Vec::new(),
            errors: Vec::new(),
            status: "Ready".into(),
            running: false,
            cancel: None,
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let text = self.url_input.read(cx).value().to_string();
        let inputs: Vec<String> = text
            .split_whitespace()
            .filter(|s| s.starts_with("http"))
            .map(String::from)
            .collect();
        if inputs.is_empty() {
            self.status = "Paste at least one http(s) link first.".into();
            cx.notify();
            return;
        }
        let dir = self.dir_input.read(cx).value().trim().to_string();
        let opts = Options {
            out_dir: if dir.is_empty() { PathBuf::from("downloads") } else { PathBuf::from(dir) },
            mode: self.mode,
            audio_format: self.audio_format,
            max_height: self.max_height,
            parallel: self.parallel,
            save_cover_file: self.save_cover_file,
            // Optional escape hatch, e.g. MEDIAGRAB_YTDLP_ARGS="--js-runtimes node"
            extra_args: std::env::var("MEDIAGRAB_YTDLP_ARGS")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default(),
            ..Options::default()
        };

        let (mut rx, cancel) = Engine::start(&self.rt, inputs, opts);
        self.cancel = Some(cancel);
        self.running = true;
        self.rows.clear();
        self.errors.clear();
        self.status = "Starting...".into();

        // Pump engine events into this view until the batch finishes.
        cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.recv().await {
                let finished = matches!(ev, Event::Finished);
                let alive = this
                    .update(cx, |this, cx| {
                        this.apply(ev);
                        cx.notify();
                    })
                    .is_ok();
                if finished || !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = &self.cancel {
            c.cancel();
            self.status = "Cancelling...".into();
            cx.notify();
        }
    }

    /// Paste whatever is on the clipboard into the link box.
    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.trim().to_string();
            self.url_input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    fn clear_links(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.url_input.update(cx, |state, cx| state.set_value("", window, cx));
        if !self.running {
            self.rows.clear();
            self.errors.clear();
            self.status = "Ready".into();
        }
        cx.notify();
    }

    /// Native folder picker; writes the chosen folder into the "Save to" box.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Choose download folder")
                .pick_folder()
                .await;
            if let Some(handle) = picked {
                let path = handle.path().to_string_lossy().into_owned();
                let _ = this.update_in(cx, |this, window, cx| {
                    this.dir_input.update(cx, |state, cx| state.set_value(path, window, cx));
                });
            }
        })
        .detach();
    }

    fn open_folder(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir_input.read(cx).value().trim().to_string();
        open_in_file_manager(&dir);
    }

    /// The whole "state machine" of the UI: one function, one match.
    fn apply(&mut self, ev: Event) {
        match ev {
            Event::Status(s) => self.status = s.into(),
            Event::Error(e) => self.errors.push(e),
            Event::ItemsFound(items) => {
                self.rows = items
                    .into_iter()
                    .map(|item| Row {
                        item,
                        state: ItemState::Queued,
                        percent: 0.0,
                        speed: None,
                        eta: None,
                    })
                    .collect();
            }
            Event::Progress { id, percent, speed, eta } => {
                // Row index == item id (the engine numbers items 0..n in order).
                if let Some(r) = self.rows.get_mut(id) {
                    if matches!(r.state, ItemState::Queued | ItemState::Downloading) {
                        r.state = ItemState::Downloading;
                    }
                    if let Some(p) = percent {
                        r.percent = p;
                    }
                    r.speed = speed;
                    r.eta = eta;
                }
            }
            Event::State { id, state } => {
                if let Some(r) = self.rows.get_mut(id) {
                    if matches!(state, ItemState::Done(_)) {
                        r.percent = 100.0;
                    }
                    r.state = state;
                }
            }
            Event::Finished => {
                self.running = false;
                self.cancel = None;
                let done = self.rows.iter().filter(|r| matches!(r.state, ItemState::Done(_))).count();
                let failed = self.rows.iter().filter(|r| matches!(r.state, ItemState::Failed(_))).count();
                let cancelled = self.rows.iter().filter(|r| r.state == ItemState::Cancelled).count();
                self.status = if self.rows.is_empty() {
                    "Nothing to download (see errors below).".into()
                } else {
                    format!("Finished: {done} done, {failed} failed, {cancelled} cancelled").into()
                };
            }
        }
    }
}

fn default_download_dir() -> String {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Downloads").join("MediaGrab").to_string_lossy().into_owned()
}

/// Reveal a folder in Explorer / Finder / the default file manager.
fn open_in_file_manager(dir: &str) {
    let path = PathBuf::from(if dir.is_empty() { "downloads" } else { dir });
    let _ = std::fs::create_dir_all(&path);
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(&path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
}

// ---------------------------------------------------------------------------
// Small reusable widgets
// ---------------------------------------------------------------------------

/// App logo drawn with plain divs (matches assets/logo.svg).
fn logo(side: f32) -> impl IntoElement {
    div()
        .size(px(side))
        .flex_shrink_0()
        .rounded(px(side * 0.28))
        .bg(brand())
        .shadow_md()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px(side * 0.52))
                .line_height(relative(1.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(WHITE))
                .child("\u{2193}"),
        )
        .child(div().mt_1().w(px(side * 0.46)).h(px(side * 0.07)).rounded_full().bg(rgb(WHITE)))
}

/// Segmented-control button: gradient/accent when active, ghost otherwise.
fn pill(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    active: bool,
    fg: Hsla,
    muted: Hsla,
) -> Stateful<Div> {
    let base = div()
        .id(id.into())
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_sm()
        .cursor_pointer()
        .child(label.into());
    if active {
        base.bg(rgb(ACCENT_A))
            .text_color(rgb(WHITE))
            .font_weight(FontWeight::SEMIBOLD)
            .shadow_sm()
    } else {
        let hover = alpha(fg, 0.08);
        base.text_color(muted).hover(move |s| s.bg(hover).text_color(fg))
    }
}

/// Big call-to-action button with the brand gradient.
fn primary_btn(id: &'static str, label: &'static str, enabled: bool, off_bg: Hsla, off_fg: Hsla) -> Stateful<Div> {
    let base = div()
        .id(id)
        .h(px(42.0))
        .px_6()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .child(label);
    if enabled {
        base.bg(brand())
            .text_color(rgb(WHITE))
            .cursor_pointer()
            .shadow_md()
            .hover(|s| s.opacity(0.92))
            .active(|s| s.opacity(0.8))
    } else {
        base.bg(off_bg).text_color(off_fg)
    }
}

/// Outlined secondary button.
fn ghost_btn(id: &'static str, label: &'static str, enabled: bool, fg: Hsla, muted: Hsla, border: Hsla) -> Stateful<Div> {
    let base = div()
        .id(id)
        .h(px(36.0))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .text_sm()
        .child(label);
    if enabled {
        let hover = alpha(fg, 0.08);
        base.text_color(fg).cursor_pointer().hover(move |s| s.bg(hover)).active(move |s| s.opacity(0.7))
    } else {
        base.text_color(alpha(muted, 0.5))
    }
}

fn progress_bar(frac: f32, track: Hsla, height: f32) -> impl IntoElement {
    div()
        .h(px(height))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(track)
        .child(div().h_full().w(relative(frac.clamp(0.0, 1.0))).rounded_full().bg(brand()))
}

fn field_label(text: &'static str, muted: Hsla) -> Div {
    div().w(px(90.0)).flex_shrink_0().text_sm().text_color(muted).child(text)
}

fn card(surface: Hsla, border: Hsla) -> Div {
    v_flex().gap_3().p_4().rounded_xl().border_1().border_color(border).bg(surface)
}

impl Render for MediaGrab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let bg = cx.theme().background;
        let green: Hsla = rgb(GREEN).into();
        let red: Hsla = rgb(RED).into();
        let accent: Hsla = rgb(ACCENT_A).into();

        // Works on light and dark themes: tint with the foreground colour.
        let surface = alpha(fg, 0.035);
        let seg_bg = alpha(fg, 0.06);
        let row_hover = alpha(fg, 0.05);

        // ---- numbers for the header / list ------------------------------
        let total = self.rows.len();
        let done = self.rows.iter().filter(|r| matches!(r.state, ItemState::Done(_))).count();
        let failed = self.rows.iter().filter(|r| matches!(r.state, ItemState::Failed(_))).count();
        let overall = if total == 0 {
            0.0
        } else {
            self.rows.iter().map(|r| r.percent.clamp(0.0, 100.0)).sum::<f32>() / (total as f32 * 100.0)
        };

        let status_color = if self.running {
            accent
        } else if self.status.starts_with("Finished") {
            if failed > 0 { red } else { green }
        } else if self.status.starts_with("Paste") || self.status.starts_with("Nothing") {
            red
        } else {
            muted
        };

        // ---- header -----------------------------------------------------
        let header = h_flex()
            .gap_3()
            .items_center()
            .child(logo(46.0))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_xl().font_weight(FontWeight::BOLD).child("MediaGrab"))
                    .child(div().text_sm().text_color(muted).child("YouTube & SoundCloud downloader")),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(alpha(status_color, 0.14))
                    .child(div().size(px(8.0)).rounded_full().bg(status_color))
                    .child(
                        div()
                            .max_w(px(360.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_sm()
                            .text_color(status_color)
                            .child(self.status.clone()),
                    ),
            );

        // ---- source card (link + folder) ---------------------------------
        let source_card = card(surface, border)
            .child(
                h_flex()
                    .gap_2()
                    .child(field_label("Link(s)", muted))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.url_input)))
                    .child(
                        ghost_btn("paste", "Paste", true, fg, muted, border)
                            .on_click(cx.listener(|this, _, window, cx| this.paste(window, cx))),
                    )
                    .child(
                        ghost_btn("clear", "Clear", true, fg, muted, border)
                            .on_click(cx.listener(|this, _, window, cx| this.clear_links(window, cx))),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(field_label("Save to", muted))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.dir_input)))
                    .child(
                        ghost_btn("browse", "Browse", true, fg, muted, border)
                            .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                    )
                    .child(
                        ghost_btn("open-folder", "Open", true, fg, muted, border)
                            .on_click(cx.listener(|this, _, _, cx| this.open_folder(cx))),
                    ),
            );

        // ---- options card -------------------------------------------------
        let mode_row = h_flex()
            .gap_2()
            .items_center()
            .child(field_label("Get", muted))
            .child(
                h_flex()
                    .gap_1()
                    .p_1()
                    .rounded_lg()
                    .bg(seg_bg)
                    .child(
                        pill("mode-audio", "Music / audio", self.mode == Mode::Audio, fg, muted).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.mode = Mode::Audio;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        pill("mode-video", "Video", self.mode == Mode::Video, fg, muted).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.mode = Mode::Video;
                                cx.notify();
                            }),
                        ),
                    ),
            );

        let detail_row = if self.mode == Mode::Audio {
            let mut seg = h_flex().gap_1().p_1().rounded_lg().bg(seg_bg);
            for f in AudioFormat::ALL {
                seg = seg.child(
                    pill(
                        format!("fmt-{}", f.as_arg()),
                        f.as_arg().to_uppercase(),
                        self.audio_format == f,
                        fg,
                        muted,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.audio_format = f;
                        cx.notify();
                    })),
                );
            }
            let cover_label = if self.save_cover_file { "\u{2713} Save cover .jpg" } else { "Save cover .jpg" };
            h_flex()
                .gap_2()
                .items_center()
                .child(field_label("Format", muted))
                .child(seg)
                .child(
                    div().ml_2().child(
                        pill("cover-file", cover_label, self.save_cover_file, fg, muted)
                            .border_1()
                            .border_color(border)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_cover_file = !this.save_cover_file;
                                cx.notify();
                            })),
                    ),
                )
        } else {
            let mut seg = h_flex().gap_1().p_1().rounded_lg().bg(seg_bg);
            for (label, h) in [("Best", None), ("1080p", Some(1080)), ("720p", Some(720)), ("480p", Some(480))] {
                seg = seg.child(
                    pill(format!("q-{label}"), label.to_string(), self.max_height == h, fg, muted).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.max_height = h;
                            cx.notify();
                        }),
                    ),
                );
            }
            h_flex().gap_2().items_center().child(field_label("Quality", muted)).child(seg)
        };

        let parallel_row = h_flex()
            .gap_2()
            .items_center()
            .child(field_label("Parallel", muted))
            .child(
                h_flex()
                    .gap_1()
                    .p_1()
                    .items_center()
                    .rounded_lg()
                    .bg(seg_bg)
                    .child(pill("par-minus", "\u{2212}", false, fg, muted).on_click(cx.listener(|this, _, _, cx| {
                        this.parallel = this.parallel.saturating_sub(1).max(1);
                        cx.notify();
                    })))
                    .child(
                        div()
                            .w(px(34.0))
                            .text_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{}", self.parallel)),
                    )
                    .child(pill("par-plus", "+", false, fg, muted).on_click(cx.listener(|this, _, _, cx| {
                        this.parallel = (this.parallel + 1).min(12);
                        cx.notify();
                    }))),
            )
            .child(div().text_sm().text_color(muted).child("downloads at the same time"));

        let options_card = card(surface, border).child(mode_row).child(detail_row).child(parallel_row);

        // ---- action bar ----------------------------------------------------
        let running = self.running;
        let actions = h_flex()
            .gap_3()
            .items_center()
            .child(
                primary_btn("download", "Download", !running, seg_bg, muted)
                    .when(!running, |b| b.on_click(cx.listener(|this, _, _, cx| this.start(cx)))),
            )
            .child(
                ghost_btn("cancel", "Cancel", running, fg, muted, border)
                    .when(running, |b| b.on_click(cx.listener(|this, _, _, cx| this.stop(cx)))),
            )
            .child(div().flex_1().min_w_0().children((total > 0).then(|| {
                v_flex()
                    .gap_1()
                    .child(progress_bar(overall, seg_bg, 6.0))
                    .child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{done} of {total} done"))
                            .child(format!("{:.0}%", overall * 100.0)),
                    )
            })));

        // ---- download list -------------------------------------------------
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let (label, color): (String, Hsla) = match &row.state {
                    ItemState::Queued => ("Queued".into(), muted),
                    ItemState::Downloading => (format!("{:.0}%", row.percent), accent),
                    ItemState::Processing => ("Converting...".into(), accent),
                    ItemState::Done(_) => ("Done".into(), green),
                    ItemState::Failed(_) => ("Failed".into(), red),
                    ItemState::Cancelled => ("Cancelled".into(), muted),
                };
                let glyph = match &row.state {
                    ItemState::Queued | ItemState::Processing => "\u{2026}",
                    ItemState::Downloading => "\u{2193}",
                    ItemState::Done(_) => "\u{2713}",
                    ItemState::Failed(_) => "\u{2715}",
                    ItemState::Cancelled => "\u{2013}",
                };
                let sub: Option<(String, Hsla)> = match &row.state {
                    ItemState::Downloading => {
                        let mut parts = Vec::new();
                        if let Some(sp) = &row.speed {
                            parts.push(sp.clone());
                        }
                        if let Some(e) = &row.eta {
                            parts.push(format!("ETA {e}"));
                        }
                        (!parts.is_empty()).then(|| (parts.join("  \u{00b7}  "), muted))
                    }
                    ItemState::Done(p) => Some((p.display().to_string(), muted)),
                    ItemState::Failed(e) => Some((e.clone(), red)),
                    _ => None,
                };
                let show_bar = matches!(row.state, ItemState::Downloading | ItemState::Processing);
                let frac = if matches!(row.state, ItemState::Processing) { 1.0 } else { row.percent / 100.0 };

                let raw_title = row.item.title.to_string();
                let title = if raw_title.trim().is_empty() || raw_title == "Untitled" {
                    format!("Item {}", i + 1)
                } else {
                    raw_title
                };

                h_flex()
                    .id(SharedString::from(format!("row-{i}")))
                    .gap_3()
                    .px_4()
                    .py_3()
                    .items_center()
                    .border_b_1()
                    .border_color(alpha(border, 0.6))
                    .hover(move |s| s.bg(row_hover))
                    .child(
                        div()
                            .size(px(32.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(alpha(color, 0.15))
                            .text_color(color)
                            .font_weight(FontWeight::BOLD)
                            .child(glyph),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(color)
                                            .child(label),
                                    ),
                            )
                            .children(show_bar.then(|| progress_bar(frac, seg_bg, 4.0)))
                            .children(sub.map(|(t, c)| {
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_xs()
                                    .text_color(c)
                                    .child(t)
                            })),
                    )
            })
            .collect();

        let list_body = if rows.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(logo(56.0))
                .child(div().mt_2().font_weight(FontWeight::SEMIBOLD).child("Nothing here yet"))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("Paste a link above and press Download (or hit Enter)."),
                )
                .into_any_element()
        } else {
            div()
                .id("downloads")
                .size_full()
                .overflow_y_scroll()
                .child(v_flex().children(rows))
                .into_any_element()
        };

        let list_header = h_flex()
            .px_4()
            .py_3()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(border)
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Downloads"))
            .child(
                h_flex()
                    .gap_3()
                    .text_sm()
                    .text_color(muted)
                    .child(format!("{total} item(s)"))
                    .children((done > 0).then(|| div().text_color(green).child(format!("{done} done"))))
                    .children((failed > 0).then(|| div().text_color(red).child(format!("{failed} failed")))),
            );

        let list_card = v_flex()
            .flex_1()
            .min_h_0()
            .rounded_xl()
            .border_1()
            .border_color(border)
            .bg(surface)
            .overflow_hidden()
            .child(list_header)
            .child(div().flex_1().min_h_0().child(list_body));

        // ---- errors ---------------------------------------------------------
        let errors_card = (!self.errors.is_empty()).then(|| {
            v_flex()
                .gap_1()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(alpha(red, 0.4))
                .bg(alpha(red, 0.08))
                .children(self.errors.iter().map(|e| div().text_xs().text_color(red).child(e.clone())))
        });

        v_flex()
            .size_full()
            .px_6()
            .py_5()
            .gap_4()
            .bg(bg)
            .text_color(fg)
            .child(header)
            .child(source_card)
            .child(options_card)
            .child(actions)
            .children(errors_card)
            .child(list_card)
            .child(
                div()
                    .text_xs()
                    .text_color(alpha(muted, 0.8))
                    .child("Only download content you own or have permission to download."),
            )
    }
}

fn main() {
    // The engine runs on tokio; keep the runtime alive for the whole program.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start tokio runtime");
    let handle = rt.handle().clone();

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx); // theme + component setup; must run before using components

        let bounds = Bounds::centered(None, size(px(920.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(780.0), px(680.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("MediaGrab".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| MediaGrab::new(handle.clone(), window, cx));
                // gpui-component requires the window's root view to be a `Root`.
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("failed to open window");

        // Quit when the last window closes (macOS keeps apps alive otherwise).
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });

    drop(rt);
}
