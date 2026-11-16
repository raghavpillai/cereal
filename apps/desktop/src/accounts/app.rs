//! The window's side of accounts: verifying each saved session at launch,
//! taking a sign-in in, a session that ends, signing out and erasing. With
//! one account these run with a list that happens to hold one.
use super::{Account, AccountId, AccountState, Blocked, VerifiedAccount, client};
use crate::app::{Cereal, Screen};
use gpui::*;
use std::{sync::mpsc, time::Duration};

/// The back-off between attempts to verify an account whose server did not
/// answer: the part uploader's, 1, 2, 4, 8, then every 15 seconds.
fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs([1, 2, 4, 8, 15][(failures.max(1) as usize - 1).min(4)])
}

/// Poll a worker thread's answer without blocking the window.
async fn answer<T: Send + 'static>(receiver: mpsc::Receiver<T>, cx: &mut AsyncApp) -> Option<T> {
    loop {
        match receiver.try_recv() {
            Ok(value) => return Some(value),
            Err(mpsc::TryRecvError::Disconnected) => return None,
            Err(mpsc::TryRecvError::Empty) => {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await
            }
        }
    }
}

impl Cereal {
    /// Clear what the window holds that belonged to whoever was signed in
    /// (or to local mode): conversations, menus, fields, the open note.
    /// Notes themselves are not touched here; each account's notes leave
    /// with its slot (`depart`).
    pub fn reset_view_state(&mut self, cx: &mut Context<Self>) {
        self.summary.dismiss();
        self.generation.cancel();
        self.server_status = None;
        self.current_note = None;
        self.chat_models.clear();
        self.selected_chat_model = None;
        self.chat_model_picker_open = false;
        self.chat_messages.clear();
        self.chat_annotations.clear();
        self.chat_history_open = false;
        self.audio_pending.clear();
        self.audio_error = None;
        self.assistant_open = false;
        self.transcript_open = false;
        self.calendars = Default::default();
        self.meeting = Default::default();
        self.title_input.update(cx, |i, cx| i.set_content("", cx));
        self.editor.update(cx, |i, cx| i.set_content("", cx));
        self.ask.update(cx, |i, cx| i.set_content("", cx));
        self.search.update(cx, |i, cx| i.set_content("", cx));
        self.search_results_query.clear();
        self.attendee_input
            .update(cx, |i, cx| i.set_content("", cx));
        self.search_open = false;
        self.menu = None;
        self.visible_menu = None;
        self.toast = None;
        self.chat_editing = None;
        self.chat_interrupted.clear();
        self.chat_thread_id = uuid::Uuid::new_v4().to_string();
        self.note_body_dirty = false;
        self.transcript_last_text.clear();
        self.transcript_last_query.clear();
        self.transcript_epoch = self.transcript_epoch.wrapping_add(1);
        self.regeneration_notice_dismissed = None;
        self.account_sheet = None;
    }

    /// Everything in memory reaches the disk, under each account's own
    /// path, before an account arrives or leaves.
    fn settle_before_account_change(&mut self, cx: &mut Context<Self>) {
        self.stop_all_chat_streams(cx);
        self.save_chat_history();
        self.finish_recording_preview(cx);
        self.flush_all_blocking(cx);
    }

    /// Why an account cannot be signed out of or removed right now.
    pub fn account_blocked(&self, account: &Account) -> Option<Blocked> {
        (self.recording.is_active()
            && self
                .recording
                .note_id
                .and_then(super::handle::slot_of)
                == Some(account.slot))
        .then_some(Blocked::Recording)
    }

    /// Verify every account that has not been verified yet, each on its own
    /// thread. An account's notes are shown only once its session has been
    /// verified in this launch.
    pub fn verify_accounts(&mut self, cx: &mut Context<Self>) {
        self.watch_accounts(cx);
        if let Some(notice) = super::take_load_notice() {
            self.show_toast(&notice, cx);
        }
        let welcoming = self.screen == Screen::Welcome;
        let accounts = super::all();
        let pending: Vec<AccountId> = accounts
            .iter()
            .filter(|account| {
                account.state == AccountState::Checking && !self.verifying.contains(&account.id)
            })
            .map(|account| account.id.clone())
            .collect();
        if !accounts.iter().any(Account::shown) && !welcoming {
            // Nobody to show notes for yet (or at all): the account page,
            // where the session is being checked or a sign-in is asked for.
            if !matches!(self.screen, Screen::Login | Screen::Link) {
                self.screen = Screen::Account;
            }
            if pending.is_empty()
                && self.verifying.is_empty()
                && !self.sign_in_asked
                && !accounts
                    .iter()
                    .any(|account| matches!(account.state, AccountState::Unreachable { .. }))
            {
                self.sign_in_asked = true;
                self.server_status = Some("Sign in to your server first".into());
                self.show_toast("Sign in to your server first", cx);
                self.resume_links(cx);
            }
        }
        for id in pending {
            self.verify_account(id, Duration::ZERO, cx);
        }
        self.settle_stray_session(cx);
    }

    fn verify_account(&mut self, id: AccountId, delay: Duration, cx: &mut Context<Self>) {
        // Each look at the registry is its own statement: the lock is never
        // held across another.
        let verification = super::registry().verification(&id);
        let Some((url, token)) = verification else {
            super::registry().session_ended(&id);
            return;
        };
        let Some(epoch) = super::epoch_of(&id) else {
            return;
        };
        self.verifying.insert(id.clone());
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let check = client::session_check(&url, &token);
            // What the server says it can do, read each time it is verified.
            let features = match &check {
                client::SessionCheck::Valid(_) => client::server_features(&url),
                _ => Vec::new(),
            };
            let _ = send.send((check, features));
        });
        cx.spawn(async move |this, cx| {
            let result = answer(receive, cx).await;
            let _ = this.update(cx, |this, cx| {
                this.verifying.remove(&id);
                if super::epoch_of(&id) != Some(epoch) {
                    // Signed out of, or signed in to again, meanwhile.
                    return;
                }
                let welcoming = this.screen == Screen::Welcome;
                match result {
                    Some((client::SessionCheck::Valid(session), features)) => {
                        this.verify_failures.remove(&id);
                        let verified = super::registry().verified(&id, *session, features);
                        match verified {
                            Ok(id) => this.account_ready(id, cx),
                            Err(message) => {
                                this.server_status = Some(message.clone());
                                if !welcoming {
                                    this.show_toast(&message, cx);
                                }
                                this.resume_links(cx);
                            }
                        }
                    }
                    Some((client::SessionCheck::Ended, _)) => {
                        // The saved session was refused. It is not tried
                        // again (for the migrated account, the legacy token
                        // is discarded).
                        super::registry().session_refused(&id);
                        let message = "Email, password or session was not accepted";
                        this.server_status = Some(message.into());
                        if !welcoming {
                            this.show_toast(message, cx);
                        }
                        this.account_needs_sign_in(&id, cx);
                        this.resume_links(cx);
                    }
                    Some((client::SessionCheck::Unknown(message), _)) => {
                        this.interrupted.insert(id.clone());
                        let failures = this.verify_failures.entry(id.clone()).or_insert(0);
                        *failures += 1;
                        let failures = *failures;
                        super::registry().set_state(
                            &id,
                            AccountState::Unreachable {
                                message: message.clone(),
                            },
                        );
                        this.server_status = Some(message.clone());
                        // Said once; the attempts after it are quiet.
                        if failures == 1 && !welcoming {
                            this.show_toast(&message, cx);
                        }
                        // Nobody is signed in: a link that was waiting to
                        // find that out says where its note is.
                        this.resume_links(cx);
                        this.verify_again(id.clone(), failures, cx);
                    }
                    None => {
                        this.server_status = Some("Session verification stopped".into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// An account whose server did not answer is asked again, on the
    /// back-off the part uploader uses, for as long as it is here.
    fn verify_again(&mut self, id: AccountId, failures: u32, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(retry_delay(failures)).await;
            let _ = this.update(cx, |this, cx| {
                let waiting = super::get(&id).is_some_and(|account| {
                    matches!(account.state, AccountState::Unreachable { .. })
                        && !account.verified_this_launch
                });
                if waiting && !this.verifying.contains(&id) {
                    super::registry().set_state(&id, AccountState::Checking);
                    this.verify_account(id.clone(), Duration::ZERO, cx);
                }
            });
        })
        .detach();
    }

    /// A legacy item an older build left (run by hand, it signed in): asked
    /// once whose session it is. A session of an account that is already
    /// here replaces that account's token; anything else is ignored and
    /// deleted. It never adds an account.
    fn settle_stray_session(&mut self, cx: &mut Context<Self>) {
        let Some((url, token)) = super::registry().take_stray() else {
            return;
        };
        let (send, receive) = mpsc::channel();
        let (asked, sent) = (url.clone(), token.clone());
        std::thread::spawn(move || {
            let _ = send.send(client::session_check(&asked, &sent));
        });
        cx.spawn(async move |this, cx| {
            let check = answer(receive, cx).await;
            let _ = this.update(cx, |this, cx| {
                let session = match check {
                    Some(client::SessionCheck::Valid(session)) => Some(*session),
                    Some(client::SessionCheck::Ended) => None,
                    // No answer: it is looked at again at the next launch.
                    _ => return,
                };
                let accepted = super::registry().settle_stray(&url, token, session);
                if let Some(accepted) = accepted {
                    for ended in accepted.ended {
                        std::thread::spawn(move || ended.client.sign_out_on_server());
                    }
                    this.account_ready(accepted.id, cx);
                }
            });
        })
        .detach();
    }

    /// An account's session has been verified (at launch, by a sign-in, or
    /// again after it had ended): its notes join the lists and everything
    /// unsent of it is queued again.
    pub fn account_ready(&mut self, id: AccountId, cx: &mut Context<Self>) {
        let Some(account) = super::get(&id) else {
            return;
        };
        self.sign_in_asked = false;
        self.verify_failures.remove(&id);
        self.withdraw_session_notification(&id);
        eprintln!("account {}: ready", id.short());
        if account.user_id.is_some()
            && let Some(paths) = super::Paths::of_environment()
        {
            if !self.workspace.by_account() {
                self.workspace = crate::model::Workspace::for_accounts(&paths);
            }
            if let Err(reason) = self.workspace.load_account(account.slot, &account.id, &paths) {
                eprintln!("account {}: workspace not loaded: {reason}", account.id.short());
                self.show_toast(
                    &format!(
                        "The notes saved on this Mac for {} could not be read",
                        account.host()
                    ),
                    cx,
                );
            }
            self.workspace.set_hidden(account.slot, false);
            // The account's threads are read from its server; nothing is read from this Mac.
            self.sync_account_chats(&account, cx);
            // Calendars hidden on this Mac before accounts get this one's id.
            let _ = crate::desktop_preferences::update_meeting(|preferences| {
                crate::calendar::migrate_hidden_calendars(
                    &mut preferences.hidden_calendars,
                    &account.id,
                );
            });
        }
        if std::env::args().any(|argument| argument == "--dark") {
            self.workspace.settings.force_dark(true);
        }
        // A sign-in made in the welcome carries on there: its next step.
        if self.screen == Screen::Welcome && self.onboarding.flow.is_some() {
            self.onboarding_signed_in(cx);
        } else if self.screen == Screen::Account {
            self.screen = Screen::Home;
        }
        self.connect_server(cx);
        // A link that was waiting for this session, or the note this
        // sign-in was started for.
        self.resume_links(cx);
        // Back after its session ended, was replaced, or its server could
        // not be reached: what it holds unsent is sent again.
        let returning = self.interrupted.remove(&id);
        self.requeue_unsent(&account, returning, cx);
        self.persist(cx);
        #[cfg(debug_assertions)]
        self.debug_view(&account, cx);
    }

    /// `CEREAL_DEBUG_VIEW=<name>` (debug builds, isolated data directory):
    /// once the first account is Ready, put one of the account screens or
    /// sheets on screen, to be looked at and captured. Nothing is signed
    /// out of or erased by it.
    #[cfg(debug_assertions)]
    fn debug_view(&mut self, account: &Account, cx: &mut Context<Self>) {
        if crate::environment::var_os("CEREAL_DATA_DIR").is_none() || self.debug_view_shown {
            return;
        }
        let Ok(view) = crate::environment::var("CEREAL_DEBUG_VIEW") else {
            return;
        };
        self.debug_view_shown = true;
        match view.as_str() {
            "accounts" | "sign-out" | "erase" | "erase-saved" | "sign-in-saved" | "sign-in-warning" => {
                self.settings_tab = crate::screens::SettingsTab::Profile;
                self.navigate(Screen::Settings, cx);
            }
            "trash" | "restore" | "restore-now" => self.open_trash(cx),
            _ => {}
        }
        // The existing dialogs, to measure the account sheets against.
        match view.as_str() {
            "shortcuts" => self.menu = Some(crate::app::Menu::Shortcuts),
            _ => {}
        }
        match view.as_str() {
            "sign-out" => self.ask_sign_out(account.id.clone(), cx),
            "erase" => self.ask_erase_account(account.id.clone(), cx),
            "sign-out-now" => {
                // What the sign-out sheet's Sign out button does.
                let id = account.id.clone();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_secs(4)).await;
                    let _ = this.update(cx, |this, cx| {
                        eprintln!("debug view: signing out");
                        this.sign_out_account(id, cx)
                    });
                })
                .detach();
            }
            "restore-now" => {
                // What the sheet's Restore button does, once the list has
                // been refreshed and the note marked.
                cx.spawn(async move |this, cx| {
                    for _ in 0..100 {
                        cx.background_executor().timer(Duration::from_millis(200)).await;
                        let done = this.update(cx, |this, cx| {
                            let marked = this
                                .workspace
                                .notes
                                .iter()
                                .find(|note| note.trashed && note.server_missing_since.is_some())
                                .map(|note| note.id);
                            match marked {
                                Some(id) => {
                                    eprintln!("debug view: restoring a note deleted on the server");
                                    this.restore_deleted_note(id, cx);
                                    true
                                }
                                None => false,
                            }
                        });
                        if !matches!(done, Ok(false)) {
                            return;
                        }
                    }
                })
                .detach();
            }
            "restore" => {
                if let Some(id) = self
                    .workspace
                    .notes
                    .iter()
                    .find(|note| note.trashed && note.server_missing_since.is_some())
                    .map(|note| note.id)
                {
                    self.restore_from_trash(id, cx);
                }
            }
            "erase-saved" | "sign-in-saved" | "sign-in-warning" => {
                // The lists are read off the UI thread: wait for them.
                cx.spawn(async move |this, cx| {
                    for _ in 0..40 {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let shown = this.update(cx, |this, cx| {
                            let Some(listing) = this.saved_data.clone() else {
                                return false;
                            };
                            this.debug_saved_sheet(&view, &listing, cx);
                            true
                        });
                        if !matches!(shown, Ok(false)) {
                            return;
                        }
                    }
                })
                .detach();
            }
            _ => {}
        }
    }

    /// Take in a sign-in the form has just finished. Other accounts, their
    /// notes, uploads and conversations are not touched (while several
    /// accounts are switched off, whoever signs in is the account, and an
    /// account that is someone else is signed out of first; its data stays).
    pub fn accept_account(&mut self, verified: VerifiedAccount, cx: &mut Context<Self>) {
        self.settle_before_account_change(cx);
        if !crate::server::enabled() {
            // Leaving "this Mac only": its notes were written under their
            // own path just now, and nothing of them follows into an account.
            self.reset_view_state(cx);
            self.chat_archives.clear();
            super::enter_server_mode();
            self.workspace = crate::model::Workspace::load();
        }
        let recording_slot = self
            .recording
            .note_id
            .filter(|_| self.recording.is_active())
            .and_then(super::handle::slot_of);
        let busy = move |account: &Account| {
            (recording_slot == Some(account.slot)).then_some(Blocked::Recording)
        };
        let accepted = super::registry().accept(verified, super::multiple(), &busy);
        let accepted = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                let message = error.message();
                self.account.update(cx, |form, cx| form.fail(message, cx));
                return;
            }
        };
        if accepted.outcome == super::registry::Outcome::TokenReplaced {
            // Work cancelled by the replacement is queued again.
            self.interrupted.insert(accepted.id.clone());
        }
        let replaced = !accepted.departed.is_empty();
        for gone in &accepted.departed {
            self.depart(gone);
        }
        if replaced {
            self.reset_view_state(cx);
        }
        for ended in accepted.ended {
            std::thread::spawn(move || ended.client.sign_out_on_server());
        }
        self.account_ready(accepted.id, cx);
        cx.notify();
    }

    /// What leaves the window with an account: its reminders, its screens,
    /// its worker, its conversations and its notes. Its slot is dead, so
    /// nothing writes its file again. Touches nothing but the window's own
    /// state.
    fn depart(&mut self, account: &Account) {
        if self
            .calendars
            .account
            .as_ref()
            .is_none_or(|owner| *owner == account.id)
        {
            self.withdraw_reminders();
            self.calendars = Default::default();
            self.meeting = Default::default();
        }
        if self
            .current_note
            .and_then(super::handle::slot_of)
            .is_some_and(|slot| slot == account.slot)
        {
            self.current_note = None;
            self.note_body_dirty = false;
        }
        self.bridges.remove(&account.id);
        self.refreshing.remove(&account.id);
        self.verifying.remove(&account.id);
        self.verify_failures.remove(&account.id);
        self.chat_archives
            .retain(|chat| chat.account.as_ref() != Some(&account.id));
        self.chat_sync.forget(&account.id);
        self.workspace.drop_slot(account.slot);
        self.caches = Default::default();
    }

    /// Sign out of an account: the session ends; everything on this Mac
    /// stays, and is here again after signing back in.
    pub fn sign_out_account(&mut self, id: AccountId, cx: &mut Context<Self>) {
        self.remove_account(id, false, cx);
    }

    /// Remove an account and erase its data from this Mac. The notes on its
    /// server are not affected.
    pub fn erase_account(&mut self, id: AccountId, cx: &mut Context<Self>) {
        self.remove_account(id, true, cx);
    }

    fn remove_account(&mut self, id: AccountId, erase: bool, cx: &mut Context<Self>) {
        let Some(account) = super::get(&id) else {
            return;
        };
        if let Some(blocked) = self.account_blocked(&account) {
            self.show_toast(blocked.message(), cx);
            return;
        }
        self.settle_before_account_change(cx);
        // In the order of the design: the epoch moves; reminders, screens
        // and notes leave and the slot is dead; the Keychain item goes; the
        // account leaves the registry; its server is told in the background.
        let directory = super::registry().directory(&id);
        let departed = {
            let mut registry = super::registry();
            registry.sign_out(&id, None, |account| self.depart(account))
        };
        if erase {
            // Written above, and never again: the slot is dead.
            crate::store::wait_idle();
            if let Some(directory) = directory {
                super::registry::erase_directory(&directory);
            }
            let _ = crate::desktop_preferences::update_meeting(|preferences| {
                crate::desktop_preferences::forget_account(preferences, &id)
            });
        }
        self.saved_data = None;
        if let Ok(Some(departed)) = departed
            && let Some(client) = departed.client
        {
            std::thread::spawn(move || client.sign_out_on_server());
        }
        crate::account::next_attempt();
        if !super::any_shown() {
            self.reset_view_state(cx);
            self.screen = Screen::Account;
        }
        cx.notify();
    }

    /// An account's own `/session` said its session has ended.
    fn account_needs_sign_in(&mut self, id: &AccountId, cx: &mut Context<Self>) {
        let Some(account) = super::get(id) else {
            return;
        };
        self.interrupted.insert(id.clone());
        eprintln!("account {}: session ended; its notes stay, nothing is sent", id.short());
        // A view filter: the notes stay loaded and are written as usual.
        self.workspace.set_hidden(account.slot, true);
        self.withdraw_reminders();
        if self.account_blocked(&account).is_some() {
            // The capture is local and continues; the note stays open and
            // Stop works. Its parts are sent after the sign-in.
            self.notify_session_ended(id, &account.host(), true, cx);
            return;
        }
        if !super::any_shown() && !matches!(self.screen, Screen::Welcome | Screen::Link | Screen::Login) {
            // With one account: the sign-in screen, as when a saved session
            // is refused at launch. Any server and any user may sign in.
            self.navigate(Screen::Account, cx);
        }
        cx.notify();
    }

    /// One long-lived watcher: when a server answers 401 or 403 to
    /// anything, that account's own `/session` is asked, once. Only its
    /// answer (a 401 with the API's own JSON body) ends the session here; a
    /// page from a proxy or a captive portal changes nothing.
    fn watch_accounts(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.accounts_watched, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let mut asking: std::collections::HashSet<AccountId> = Default::default();
            let (send, receive) = mpsc::channel::<(AccountId, u64, client::SessionCheck)>();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                for id in super::take_suspects() {
                    if !asking.insert(id.clone()) {
                        continue;
                    }
                    let verification = super::registry().verification(&id);
                    let Some((url, token)) = verification else {
                        asking.remove(&id);
                        continue;
                    };
                    let Some(epoch) = super::epoch_of(&id) else {
                        asking.remove(&id);
                        continue;
                    };
                    let send = send.clone();
                    std::thread::spawn(move || {
                        let _ = send.send((id, epoch, client::session_check(&url, &token)));
                    });
                }
                while let Ok((id, epoch, check)) = receive.try_recv() {
                    asking.remove(&id);
                    let ended = matches!(check, client::SessionCheck::Ended)
                        && super::epoch_of(&id) == Some(epoch);
                    if !ended {
                        continue;
                    }
                    let alive = this.update(cx, |this, cx| {
                        super::registry().session_ended(&id);
                        let host = super::get(&id).map(|account| account.host()).unwrap_or_default();
                        let blocked = super::get(&id).is_some_and(|account| this.account_blocked(&account).is_some());
                        // While recording, `account_needs_sign_in` says it.
                        if !blocked {
                            this.notify_session_ended(&id, &host, false, cx);
                        }
                        this.account_needs_sign_in(&id, cx);
                    });
                    if alive.is_err() {
                        return;
                    }
                }
                #[cfg(debug_assertions)]
                if this.update(cx, |this, cx| this.debug_sign_in(cx)).is_err() {
                    return;
                }
                if this.update(cx, |_, _| ()).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// `CEREAL_DEBUG_SIGN_IN_FILE=<file>` (debug builds, isolated data
    /// directory): once an account needs a sign-in and the file exists, the
    /// session in it (`{"url","token"}`) is verified and taken in exactly as
    /// one from the sign-in form is (`accept_account`). The file is removed
    /// when it is read. For checking a session that ends mid-recording
    /// without typing a password into a window.
    #[cfg(debug_assertions)]
    fn debug_sign_in(&mut self, cx: &mut Context<Self>) {
        if crate::environment::var_os("CEREAL_DATA_DIR").is_none() {
            return;
        }
        let Some(file) = crate::environment::var_os("CEREAL_DEBUG_SIGN_IN_FILE") else {
            return;
        };
        let accounts = super::all();
        let waiting = accounts.is_empty()
            || accounts
                .iter()
                .any(|account| account.state == AccountState::NeedsSignIn);
        if !waiting {
            return;
        }
        let Ok(bytes) = std::fs::read(&file) else {
            return;
        };
        let _ = std::fs::remove_file(&file);
        let Some(saved) = super::keychain::SavedSession::decode(&bytes) else {
            return;
        };
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let check = client::session_check(&saved.url, &saved.token);
            let _ = send.send((saved, check));
        });
        cx.spawn(async move |this, cx| {
            let Some((saved, check)) = answer(receive, cx).await else {
                return;
            };
            let client::SessionCheck::Valid(session) = check else {
                eprintln!("debug sign-in: the session was not accepted");
                return;
            };
            let _ = this.update(cx, |this, cx| {
                eprintln!("debug sign-in: taking the session in");
                this.accept_account(
                    VerifiedAccount {
                        url: saved.url,
                        token: saved.token,
                        session: *session,
                    },
                    cx,
                )
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; this is a plain unit test.
    use std::prelude::v1::test;

    #[test]
    fn an_unreachable_account_is_asked_again_on_the_uploaders_back_off() {
        let delays: Vec<u64> = (1..=7).map(|n| retry_delay(n).as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 15, 15, 15]);
    }
}
