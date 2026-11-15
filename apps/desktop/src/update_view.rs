//! "Check for updates": the menu item, the row of Settings → About and the
//! notice that follows an update from the question to the restart. The work
//! itself is in `update.rs`.
//!
//! What ends at once (up to date, a failure) is said in a toast. What waits
//! for the person or takes a while (a version is available, downloading,
//! verifying, restart to finish) is a notice in the toast's place and face,
//! which stays until it is answered.
use crate::{
    app::Cereal,
    ui::*,
    update::{self, Check, Offer, Problem},
};
use gpui::{prelude::*, *};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

/// A verified update waiting for the restart that installs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    version: String,
    /// The unpacked, verified app.
    app: PathBuf,
    /// The running app it replaces, and the team that signed both.
    bundle: PathBuf,
    team: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Available(Offer),
    Downloading(Offer),
    Verifying(Offer),
    Ready(Staged),
    Restarting,
}

/// What the notice says: a title, a line under it, whether work is going
/// on, and the action it offers beside "Later".
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Notice {
    title: String,
    detail: Option<String>,
    busy: bool,
    action: Option<&'static str>,
}

#[derive(Default)]
pub struct Updates {
    pub phase: Phase,
    /// How the last check or update ended, for the row in Settings.
    outcome: Option<String>,
    /// Bytes of the archive on disk so far.
    received: Arc<AtomicU64>,
    /// "Later" puts the notice away; the row in Settings keeps the offer.
    put_away: bool,
    /// The notice last drawn, kept so it can finish leaving.
    shown: Notice,
    /// Counts the checks and updates started, so the end of one that has
    /// been replaced is dropped.
    turn: u64,
}

/// Megabytes with one decimal, the way a download is counted.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.)
}

/// "4.2 MB of 9.8 MB (43%)". The total is the one the server advertised.
pub fn progress_text(received: u64, total: u64) -> String {
    let received = received.min(total);
    let percent = if total == 0 { 0 } else { received * 100 / total };
    format!("{} of {} ({percent}%)", megabytes(received), megabytes(total))
}

impl Updates {
    fn notice(&self) -> Option<Notice> {
        let version = update::current_version();
        Some(match &self.phase {
            Phase::Idle => return None,
            Phase::Checking => Notice {
                title: "Checking for updates…".into(),
                busy: true,
                ..Default::default()
            },
            Phase::Available(offer) => Notice {
                title: format!("Version {} is available", offer.version),
                detail: Some(format!("You have {version}.")),
                action: Some("Update"),
                ..Default::default()
            },
            Phase::Downloading(offer) => Notice {
                title: format!("Downloading version {}", offer.version),
                detail: Some(progress_text(
                    self.received.load(Ordering::Acquire),
                    offer.size,
                )),
                busy: true,
                ..Default::default()
            },
            Phase::Verifying(_) => Notice {
                title: "Verifying the update…".into(),
                busy: true,
                ..Default::default()
            },
            Phase::Ready(staged) => Notice {
                title: "Restart to finish".into(),
                detail: Some(format!("Version {} is ready.", staged.version)),
                action: Some("Restart"),
                ..Default::default()
            },
            Phase::Restarting => Notice {
                title: "Restarting…".into(),
                busy: true,
                ..Default::default()
            },
        })
    }

    /// Whether the notice is on screen. While it is, what it says is kept,
    /// so it can still be drawn as it leaves.
    pub fn notice_open(&mut self) -> bool {
        let open = self.open();
        if let Some(notice) = self.notice().filter(|_| open) {
            self.shown = notice;
        }
        open
    }

    fn open(&self) -> bool {
        self.phase != Phase::Idle && !(self.put_away && self.waits_for_an_answer())
    }

    fn waits_for_an_answer(&self) -> bool {
        matches!(self.phase, Phase::Available(_) | Phase::Ready(_))
    }

    /// The sentence under "Version" in Settings, and the button beside it:
    /// its label, and whether it can be pressed now.
    fn row(&self) -> (String, &'static str, bool) {
        let notice = self.notice();
        let sentence = match (&self.phase, &notice) {
            (Phase::Idle, _) | (_, None) => self
                .outcome
                .clone()
                .unwrap_or_else(|| "Native GPUI build.".into()),
            (Phase::Downloading(_), Some(notice)) => format!(
                "{}: {}",
                notice.title,
                notice.detail.clone().unwrap_or_default()
            ),
            (_, Some(notice)) => notice.title.clone(),
        };
        let (label, enabled) = match &self.phase {
            Phase::Idle => ("Check for updates", true),
            Phase::Checking => ("Checking…", false),
            Phase::Available(_) => ("Update", true),
            Phase::Downloading(_) => ("Downloading…", false),
            Phase::Verifying(_) => ("Verifying…", false),
            Phase::Ready(_) => ("Restart", true),
            Phase::Restarting => ("Restarting…", false),
        };
        (sentence, label, enabled)
    }
}

/// What the worker that downloads and verifies an update reports.
enum Progress {
    Verifying,
    Done(Result<Staged, Problem>),
}

/// Download the offered archive, check it, unpack it and check the app
/// inside. Nothing of the installed app is touched here.
fn fetch_and_verify(
    server: &str,
    offer: &Offer,
    received: &AtomicU64,
    report: &mpsc::Sender<Progress>,
) -> Result<Staged, Problem> {
    let (bundle, team) = update::readiness()?;
    let staged = (|| {
        let archive = update::download(server, offer, received)?;
        let _ = report.send(Progress::Verifying);
        let app = update::unpack(&archive)?;
        update::verify_bundle(&app, offer, &team)?;
        Ok(Staged {
            version: offer.version.clone(),
            app,
            bundle,
            team,
        })
    })();
    if staged.is_err() {
        // Nothing that was refused stays on disk.
        update::discard_download();
    }
    staged
}

impl Cereal {
    /// The menu item and the button in Settings. With an answer already
    /// waiting, it brings that answer back instead of asking again.
    pub fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.updates.put_away = false;
        if self.updates.phase != Phase::Idle {
            cx.notify();
            return;
        }
        let server = crate::account::server_origin().filter(|_| crate::server::enabled());
        let Some(server) = server else {
            self.end_update(Err(Problem::NotConnected), cx);
            return;
        };
        self.updates.phase = Phase::Checking;
        self.updates.outcome = None;
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(update::check(&server));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => break Err(Problem::Unreachable),
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(60))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if this.updates.turn != turn {
                    return;
                }
                match result {
                    Ok(Check::Available(offer)) => {
                        this.updates.phase = Phase::Available(offer);
                        cx.notify();
                    }
                    Ok(Check::UpToDate) => this.end_update(Ok(()), cx),
                    Err(problem) => this.end_update(Err(problem), cx),
                }
            });
        })
        .detach();
    }

    /// A check or an update is over: say how it ended.
    fn end_update(&mut self, result: Result<(), Problem>, cx: &mut Context<Self>) {
        let message = match result {
            Ok(()) => format!("Cereal is up to date ({})", update::current_version()),
            Err(problem) => problem.message(),
        };
        self.updates.phase = Phase::Idle;
        self.updates.outcome = Some(message.clone());
        self.show_toast(&message, cx);
    }

    /// The notice's action, and the button in Settings: update when a
    /// version is available, restart when one is ready, otherwise check.
    pub fn advance_update(&mut self, cx: &mut Context<Self>) {
        match self.updates.phase.clone() {
            Phase::Available(offer) => self.download_update(offer, cx),
            Phase::Ready(staged) => self.install_update(staged, cx),
            _ => self.check_for_updates(cx),
        }
    }

    fn download_update(&mut self, offer: Offer, cx: &mut Context<Self>) {
        let Some(server) = crate::account::server_origin().filter(|_| crate::server::enabled())
        else {
            self.end_update(Err(Problem::NotConnected), cx);
            return;
        };
        self.updates.put_away = false;
        self.updates.received = Arc::new(AtomicU64::new(0));
        self.updates.phase = Phase::Downloading(offer.clone());
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        let received = self.updates.received.clone();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let result = fetch_and_verify(&server, &offer, &received, &send);
            let _ = send.send(Progress::Done(result));
        });
        cx.spawn(async move |this, cx| {
            loop {
                let step = match receive.try_recv() {
                    Ok(step) => Some(step),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Progress::Done(Err(Problem::Download)))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                };
                let finished = matches!(step, Some(Progress::Done(_)));
                let current = this.update(cx, |this, cx| {
                    if this.updates.turn != turn {
                        return false;
                    }
                    match step {
                        Some(Progress::Verifying) => {
                            if let Phase::Downloading(offer) = this.updates.phase.clone() {
                                this.updates.phase = Phase::Verifying(offer);
                            }
                        }
                        Some(Progress::Done(Ok(staged))) => {
                            this.updates.phase = Phase::Ready(staged);
                        }
                        Some(Progress::Done(Err(problem))) => {
                            this.end_update(Err(problem), cx);
                        }
                        // The progress figure follows the bytes on disk.
                        None => {}
                    }
                    cx.notify();
                    true
                });
                if finished || !current.unwrap_or(false) {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
        })
        .detach();
    }

    /// "Restart": put the verified app in the running one's place, then
    /// quit and let the helper start the new one. A swap that fails leaves
    /// the installed app as it was, and the app carries on.
    fn install_update(&mut self, staged: Staged, cx: &mut Context<Self>) {
        if self.recording.is_active() {
            self.show_toast("Stop the recording before restarting", cx);
            return;
        }
        self.updates.put_away = false;
        self.updates.phase = Phase::Restarting;
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        let (send, receive) = mpsc::channel();
        let bundle = staged.bundle.clone();
        std::thread::spawn(move || {
            let _ = send.send(update::install(&staged.app, &staged.bundle, &staged.team));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => break Err(Problem::Install),
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(40))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if this.updates.turn != turn {
                    return;
                }
                match result {
                    Ok(()) => {
                        // The new app is in place. If the helper cannot be
                        // started the person opens it themselves.
                        let _ = update::relaunch_after_exit(&bundle);
                        this.quit_after_audio_stop(cx);
                    }
                    Err(problem) => this.end_update(Err(problem), cx),
                }
            });
        })
        .detach();
    }

    /// "Later", and the notice's close button.
    fn put_update_away(&mut self, cx: &mut Context<Self>) {
        self.updates.put_away = true;
        cx.notify();
    }

    /// The first row of Settings → About: the version this app is, how the
    /// last check went, and the button that checks, updates or restarts.
    pub fn render_version_row(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let (sentence, label, enabled) = self.updates.row();
        crate::screens::settings_row("cereal", "Version", sentence, t)
            .child(small(update::current_version(), t).flex_shrink_0())
            .child(
                pill("settings-check-updates", label, "", t)
                    .flex_shrink_0()
                    .when(!enabled, |pill| pill.opacity(0.5).cursor_default())
                    .when(enabled, |pill| {
                        pill.on_click(cx.listener(|this, _, _, cx| this.advance_update(cx)))
                    }),
            )
    }

    /// The notice, in the toast's corner and face. `open` is its presence;
    /// while it leaves, the notice last shown is drawn.
    pub fn render_update_notice(
        &self,
        pose: crate::motion::Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        let notice = self.updates.shown.clone();
        crate::motion::pop(
            div()
                .id("update-notice")
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .w(px(384.))
                .max_w(window.viewport_size().width - px(32.))
                .min_h(px(NOTICE_HEIGHT))
                .pl(px(16.))
                .pr(px(if notice.action.is_some() { 12. } else { 8. }))
                .py(px(10.))
                .rounded(px(12.))
                .bg(t.card)
                .border_1()
                .border_color(t.border)
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .items_center()
                .gap(px(12.))
                .child(if notice.busy {
                    crate::motion::spinner(t.muted)
                } else {
                    div()
                        .size(px(16.))
                        .flex_shrink_0()
                        .rounded_full()
                        .border_1()
                        .border_color(t.muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .text_color(t.muted)
                        .child("i")
                        .into_any_element()
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.text)
                                .child(notice.title.clone()),
                        )
                        .when_some(notice.detail.clone(), |d, detail| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(t.muted)
                                    .child(detail),
                            )
                        }),
                )
                .when_some(notice.action, |d, action| {
                    d.child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                pill("update-later", "Later", "", t).on_click(
                                    cx.listener(|this, _, _, cx| this.put_update_away(cx)),
                                ),
                            )
                            .child(
                                pill("update-action", action, "", t)
                                    .bg(t.text)
                                    .text_color(t.bg)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.advance_update(cx)),
                                    ),
                            ),
                    )
                }),
            crate::motion::Pop::chip((1., 1.)),
            pose,
        )
    }
}

/// The notice's height with one or two lines of text, which is the toast's:
/// a toast that appears while the notice is up sits this far above it.
pub const NOTICE_HEIGHT: f32 = 56.;

#[cfg(test)]
mod tests {
    // Named one by one: a glob would bring in gpui's own `test` attribute.
    use super::{Phase, Staged, Updates, progress_text};
    use crate::update::Offer;
    use std::{path::PathBuf, sync::atomic::Ordering};

    fn offer() -> Offer {
        Offer {
            version: "0.2.0".into(),
            path: "/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip".into(),
            size: 9_800_000,
            sha256: "0".repeat(64),
        }
    }

    #[test]
    fn progress_is_counted_against_the_advertised_size() {
        assert_eq!(progress_text(0, 9_800_000), "0.0 MB of 9.8 MB (0%)");
        assert_eq!(progress_text(4_214_000, 9_800_000), "4.2 MB of 9.8 MB (43%)");
        assert_eq!(progress_text(9_800_000, 9_800_000), "9.8 MB of 9.8 MB (100%)");
        // Never more than the whole, and no division by nothing.
        assert_eq!(progress_text(12_000_000, 9_800_000), "9.8 MB of 9.8 MB (100%)");
        assert_eq!(progress_text(5, 0), "0.0 MB of 0.0 MB (0%)");
    }

    #[test]
    fn every_phase_has_its_notice_and_its_button() {
        let mut updates = Updates::default();
        assert_eq!(updates.notice(), None);
        assert!(!updates.open());
        assert_eq!(updates.row(), ("Native GPUI build.".into(), "Check for updates", true));
        updates.outcome = Some("Cereal is up to date (0.1.0)".into());
        assert_eq!(updates.row().0, "Cereal is up to date (0.1.0)");

        updates.phase = Phase::Checking;
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Checking for updates…");
        assert!(notice.busy && notice.action.is_none());
        assert_eq!(updates.row(), ("Checking for updates…".into(), "Checking…", false));

        updates.phase = Phase::Available(offer());
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Version 0.2.0 is available");
        assert_eq!(notice.action, Some("Update"));
        assert!(!notice.busy);
        assert_eq!(updates.row().1, "Update");

        updates.phase = Phase::Downloading(offer());
        updates.received.store(4_214_000, Ordering::Release);
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Downloading version 0.2.0");
        assert_eq!(notice.detail.as_deref(), Some("4.2 MB of 9.8 MB (43%)"));
        assert_eq!(
            updates.row(),
            (
                "Downloading version 0.2.0: 4.2 MB of 9.8 MB (43%)".into(),
                "Downloading…",
                false
            )
        );

        updates.phase = Phase::Verifying(offer());
        assert_eq!(updates.notice().unwrap().title, "Verifying the update…");
        assert_eq!(updates.row().1, "Verifying…");

        updates.phase = Phase::Ready(Staged {
            version: "0.2.0".into(),
            app: PathBuf::from("/tmp/update/unpacked/Cereal.app"),
            bundle: PathBuf::from("/Applications/Cereal.app"),
            team: "LZWN2FF7YB".into(),
        });
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Restart to finish");
        assert_eq!(notice.action, Some("Restart"));
        assert_eq!(updates.row().1, "Restart");
    }

    #[test]
    fn later_puts_away_only_a_notice_that_waits_for_an_answer() {
        let mut updates = Updates {
            phase: Phase::Available(offer()),
            ..Default::default()
        };
        assert!(updates.open());
        updates.put_away = true;
        assert!(!updates.open());
        // The offer is still there for the button in Settings.
        assert_eq!(updates.row().1, "Update");
        // Work that is going on is always shown.
        updates.phase = Phase::Downloading(offer());
        assert!(updates.open());
        updates.phase = Phase::Idle;
        assert!(!updates.open());
    }
}
