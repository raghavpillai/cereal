mod account;
mod accounts;
mod app;
mod audio;
mod calendar;
mod chat;
mod chrome;
mod desktop_preferences;
mod device_settings;
mod environment;
mod fetch;
mod folders;
mod handoff;
mod home;
mod input;
mod lazy;
mod link;
mod live;
mod live_view;
mod login;
mod meeting;
mod menubar;
mod model;
mod motion;
mod note;
mod onboarding;
mod onboarding_scene;
mod onboarding_view;
mod pill;
mod recording;
mod recording_view;
mod screens;
mod server;
mod shared;
mod shared_view;
mod smoke;
mod store;
mod summary;
mod tour;
mod ui;
mod update;
mod update_view;
#[cfg(test)]
mod validation;
use gpui::*;
fn main() {
    // `CEREAL_KEYCHAIN_PROBE=1`: milestone P0's check of the Keychain on an
    // installed, signed build (see `accounts::keychain::probe`). It opens
    // no window and prints only what happened.
    if environment::var("CEREAL_KEYCHAIN_PROBE").is_ok_and(|value| value == "1") {
        std::process::exit(if accounts::keychain::probe() { 0 } else { 1 });
    }
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(result) = audio::probe_from_args(&arguments) {
        match result {
            Ok(success) => std::process::exit(if success { 0 } else { 1 }),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }
    let application = Application::new().with_assets(ui::Assets);
    // `cereal://` URLs: a shared note, a note of one's own, the answer to a
    // sign-in in the browser. One that launches the app arrives before the
    // window exists and is read once it does.
    application.on_open_urls(shared_view::receive);
    application.on_reopen(|cx| {
        if pill::reopen_preview(cx) {
            return;
        }
        if let Some(handle) = cx
            .windows()
            .into_iter()
            .find_map(|w| w.downcast::<app::Cereal>())
        {
            let _ = handle.update(cx, |_, window, cx| {
                pill::reveal_main_window(window);
                window.activate_window();
                cx.activate(true);
            });
        }
    });
    application.run(|cx: &mut App| {
        ui::init(cx);
        if let Err(error) = desktop_preferences::apply_saved_icon() {
            eprintln!("{error}");
        }
        input::init(cx);
        app::init(cx);
        onboarding_view::init(cx);
        cx.set_menus(vec![Menu {
            name: "Cereal".into(),
            items: vec![
                MenuItem::action("Check for Updates…", app::CheckForUpdates),
                MenuItem::separator(),
                MenuItem::action("New Note", app::NewNote),
                MenuItem::action("Search Notes", app::Search),
                MenuItem::action("Close Window", app::CloseWindow),
                MenuItem::action("Hide Cereal", app::Hide),
                MenuItem::separator(),
                MenuItem::action("Quit", app::Quit),
                MenuItem::action("Quit Completely", app::QuitCompletely),
            ],
        }]);
        let compact = std::env::args().any(|a| a == "--compact");
        let bounds = Bounds::centered(
            None,
            if compact {
                size(px(760.), px(640.))
            } else {
                size(px(1200.), px(840.))
            },
            cx,
        );
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Cereal".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(15.), px(15.))),
                    }),
                    window_min_size: Some(size(px(760.), px(580.))),
                    app_id: Some("dev.cereal.desktop".into()),
                    ..Default::default()
                },
                |window, cx| {
                    let entity = cx.new(app::Cereal::new);
                    window.focus(&entity.focus_handle(cx));
                    entity.update(cx, |view, cx| {
                        view.connect_server(cx);
                        view.workspace.settings.dark_mode = std::env::args().any(|a| a == "--dark")
                            || view.workspace.settings.dark_mode;
                        let mode = std::env::args()
                            .find_map(|a| a.strip_prefix("--view=").map(str::to_string))
                            .unwrap_or_default();
                        match mode.as_str() {
                            "login" => view.navigate(app::Screen::Login, cx),
                            "login-sso" => {
                                view.navigate(app::Screen::Login, cx);
                                view.login.update(cx, |login, cx| login.reveal_sso(cx));
                            }
                            "login-waiting" => {
                                view.navigate(app::Screen::Login, cx);
                                view.login
                                    .update(cx, |login, cx| login.start_provider("Google", cx));
                            }
                            "recording-error" => {
                                view.open_note(1, cx);
                                view.recording.begin(1);
                                view.recording.fail(view.recording.epoch);
                                view.transcript_open = true;
                                view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
                            }
                            "recording-live" | "pill" | "pill-expanded" | "pill-attention" => {
                                view.open_note(1, cx);
                                view.start_recording_preview(1, cx);
                                if mode == "pill-attention" {
                                    view.recording.set_transcription_attention(
                                        recording::TranscriptionAttention::Warning,
                                    );
                                }
                            }
                            "summary" | "summary-loading" => {
                                view.open_note(1, cx);
                                view.active_tab = app::NoteTab::Enhanced;
                                view.workspace.settings.dark_mode = true;
                                let paragraph = view
                                    .workspace
                                    .note(1)
                                    .and_then(|note| note.enhanced.lines().nth(12))
                                    .unwrap_or("")
                                    .trim_start_matches(['-', ' '])
                                    .to_string();
                                view.open_summary(
                                    1,
                                    12,
                                    paragraph,
                                    Bounds::new(point(px(904.), px(300.)), size(px(24.), px(24.))),
                                    cx,
                                );
                                if mode == "summary-loading" {
                                    view.summary.epoch = view.summary.epoch.wrapping_add(1);
                                } else if let Some(open) = view.summary.open.as_mut() {
                                    open.ready = true;
                                }
                            }
                            "note" => view.open_note(1, cx),
                            "edit" => {
                                view.open_note(1, cx);
                                view.active_tab = app::NoteTab::Notes;
                            }
                            "chat" => view.navigate(app::Screen::Chat, cx),
                            "connect" => view.navigate(app::Screen::Connect, cx),
                            "settings" => view.navigate(app::Screen::Settings, cx),
                            "shared" => view.navigate(app::Screen::Shared, cx),
                            "search" => view.open_search(window, cx),
                            _ => {}
                        }
                        view.assistant_open = std::env::args().any(|a| a == "--assistant");
                    });
                    entity
                },
            )
            .unwrap();
        cx.activate(true);
        let mode = std::env::args()
            .find_map(|a| a.strip_prefix("--view=").map(str::to_string))
            .unwrap_or_default();
        // `CEREAL_CAPTURE_PROBE=<seconds>` measures a short real capture
        // (see `tour::capture_probe`); the pill is shown throughout so its
        // windows are measured too.
        let probe = crate::environment::var("CEREAL_CAPTURE_PROBE")
            .ok()
            .and_then(|seconds| seconds.parse::<u64>().ok());
        pill::install(handle, if probe.is_some() { "pill" } else { &mode }, cx);
        store::install(handle, cx);
        menubar::install(handle, cx);
        shared_view::install(handle, cx);
        // What an earlier update left behind is cleared away.
        update::tidy();
        if let Some(seconds) = probe {
            assert!(
                crate::environment::var_os("CEREAL_DATA_DIR").is_some(),
                "The capture probe requires an isolated CEREAL_DATA_DIR"
            );
            tour::capture_probe(handle, seconds, cx);
        }
        // `CEREAL_LIVE_PROBE=<seconds>` records from a fixture through the
        // live transcript stream against the configured server (see
        // `tour::live_probe`).
        if let Some(seconds) = crate::environment::var("CEREAL_LIVE_PROBE")
            .ok()
            .and_then(|seconds| seconds.parse::<u64>().ok())
        {
            assert!(
                crate::environment::var_os("CEREAL_DATA_DIR").is_some()
                    && crate::environment::var_os("CEREAL_CAPTURE_FIXTURE").is_some(),
                "The live probe requires an isolated CEREAL_DATA_DIR and a CEREAL_CAPTURE_FIXTURE"
            );
            tour::live_probe(handle, seconds, cx);
        }
        // `CEREAL_FUNCTIONAL_LOG=1` prints what the app is doing as it changes
        // (see `tour::functional_log`); the app is driven from outside.
        if crate::environment::var_os("CEREAL_FUNCTIONAL_LOG").is_some() {
            assert!(
                crate::environment::var_os("CEREAL_DATA_DIR").is_some(),
                "The functional log requires an isolated CEREAL_DATA_DIR"
            );
            tour::functional_log(handle, cx);
        }
        // `CEREAL_WELCOME_TOUR=<name>` walks the first-run welcome by itself
        // (see `tour::welcome`), for captures. Debug builds only.
        #[cfg(debug_assertions)]
        if let Ok(name) = crate::environment::var("CEREAL_WELCOME_TOUR") {
            assert!(
                crate::environment::var_os("CEREAL_DATA_DIR").is_some(),
                "The welcome tour requires an isolated CEREAL_DATA_DIR"
            );
            tour::welcome(handle, name, cx);
        }
        if std::env::args().any(|a| a == "--motion-tour")
            || (cfg!(debug_assertions) && crate::environment::var_os("CEREAL_AUDIT").is_some())
        {
            assert!(
                crate::environment::var_os("CEREAL_DATA_DIR").is_some(),
                "The motion tour requires an isolated CEREAL_DATA_DIR"
            );
            tour::run(handle, cx);
        }
        if std::env::args().any(|a| a == "--smoke-test") {
            assert!(
                crate::environment::var_os("CEREAL_DATA_DIR").is_some(),
                "Smoke tests require an isolated CEREAL_DATA_DIR"
            );
            smoke::run(handle, cx);
        }
    });
}
