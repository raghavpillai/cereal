//! Durable saves that never run on the typing path.
//!
//! Editing marks the workspace dirty and arms one timer. When the timer fires
//! the workspace is serialised once on the UI thread and handed to a
//! background write, which keeps the existing atomic write-and-rename. So:
//!
//! * a burst of keystrokes costs one write, holding the final content;
//! * at any moment everything older than [`SAVE_WINDOW`] plus one write is on
//!   disk, so a crash loses at most that window;
//! * leaving a note, deactivating or closing the window and stopping a
//!   recording write straight away without waiting for the timer;
//! * quitting, switching account, an explicit Save and system sleep write
//!   synchronously and wait for any write still in flight;
//! * [`Workspace::load`](crate::model::Workspace::load) waits for writes in
//!   flight, so a read can never see the file older than what was handed over.
//!
//! Writes are serialised by one lock and ordered by generation, so a slow
//! write can never land on top of a newer one.
use crate::app::Cereal;
use gpui::*;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

/// How long an edit may wait before it is written. A crash loses at most
/// this much typing (plus the write itself).
pub const SAVE_WINDOW: Duration = Duration::from_millis(300);

/// UI-thread bookkeeping for one debounced file.
#[derive(Debug, Default)]
pub struct Autosave {
    dirty: bool,
    armed: bool,
}

impl Autosave {
    /// Record an edit. Returns true when the caller must arm the timer: the
    /// first edit of a window does, later edits ride the same timer.
    pub fn touch(&mut self) -> bool {
        self.dirty = true;
        !std::mem::replace(&mut self.armed, true)
    }

    /// The timer fired. Returns true when there is something to write.
    pub fn fire(&mut self) -> bool {
        self.armed = false;
        std::mem::take(&mut self.dirty)
    }

    /// Write now instead of waiting. Returns true when there is something to
    /// write; a timer that is still armed will then find nothing to do.
    pub fn flush(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// A write failed: its content is still only in memory, so the next
    /// edit, flush or quit must try again.
    pub fn retry(&mut self) {
        self.dirty = true;
    }

    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[derive(Default)]
struct Writer {
    /// Newest generation written per file.
    written: HashMap<PathBuf, u64>,
    /// Files this process has written successfully, so their previous content
    /// is known to be ours and need not be re-validated before each write.
    verified: HashSet<PathBuf>,
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);

/// File writes completed so far, for `CEREAL_FRAME_STATS`.
pub fn writes() -> u64 {
    WRITES.load(Ordering::Relaxed)
}
static WRITER: Mutex<Option<Writer>> = Mutex::new(None);
static IN_FLIGHT: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// A ticket for one write. Take it on the UI thread at the moment the content
/// is captured; later tickets always win over earlier ones.
pub fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

/// One write that has been handed over. It counts as in flight until it is
/// dropped: when its [`commit`] returns, or if the job carrying it is
/// discarded without ever running, so a cancelled write can never leave
/// readers waiting.
pub struct Ticket(());

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut pending = IN_FLIGHT
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *pending = pending.saturating_sub(1);
        IN_FLIGHT.1.notify_all();
    }
}

/// Count a write as handed over. Move the ticket into the job that performs
/// the write and pass it to [`commit`].
#[must_use]
pub fn begin() -> Ticket {
    *IN_FLIGHT
        .0
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) += 1;
    Ticket(())
}

/// Block until every handed-over write has finished.
pub fn wait_idle() {
    let mut pending = IN_FLIGHT
        .0
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    while *pending > 0 {
        pending = IN_FLIGHT
            .1
            .wait(pending)
            .unwrap_or_else(|poison| poison.into_inner());
    }
}

/// Run one write, serialised with every other and skipped when a newer
/// generation of the same file is already on disk. `write` is told whether
/// this is the first write to the file in this process, when whatever is on
/// disk has to be treated as unknown. Returns whether it wrote.
pub fn commit(
    path: &Path,
    generation: u64,
    _ticket: Ticket,
    write: impl FnOnce(bool) -> Result<(), String>,
) -> Result<bool, String> {
    {
        let mut guard = WRITER.lock().unwrap_or_else(|poison| poison.into_inner());
        let writer = guard.get_or_insert_with(Writer::default);
        if writer
            .written
            .get(path)
            .is_some_and(|newest| *newest >= generation)
        {
            Ok(false)
        } else {
            let first = !writer.verified.contains(path);
            write(first).map(|()| {
                WRITES.fetch_add(1, Ordering::Relaxed);
                writer.written.insert(path.to_path_buf(), generation);
                writer.verified.insert(path.to_path_buf());
                true
            })
        }
    }
}

/// Chat history that has been serialised and is waiting for a write.
type PendingHistory = Option<(PathBuf, Vec<u8>, u64)>;
static CHAT_HISTORY: Mutex<PendingHistory> = Mutex::new(None);
static CHAT_HISTORY_SAVED: AtomicU64 = AtomicU64::new(0);

fn fingerprint(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish() | 1
}

/// Queue encoded chat history for writing unless it is byte-for-byte what was
/// last queued. Returns whether anything was queued.
pub fn queue_chat_history(path: PathBuf, data: Vec<u8>) -> bool {
    let print = fingerprint(&data);
    if CHAT_HISTORY_SAVED.swap(print, Ordering::SeqCst) == print {
        return false;
    }
    *CHAT_HISTORY.lock().unwrap() = Some((path, data, next_generation()));
    true
}

fn take_chat_history() -> PendingHistory {
    CHAT_HISTORY.lock().unwrap().take()
}

impl Cereal {
    /// Record a change to the workspace. Cheap: the write happens once, up to
    /// [`SAVE_WINDOW`] later, off the UI thread.
    pub fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.autosave.touch() {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SAVE_WINDOW).await;
                let _ = this.update(cx, |this, cx| {
                    if this.autosave.fire() {
                        this.write_workspace(false, cx);
                    }
                });
            })
            .detach();
        }
    }

    /// Hand pending edits to the background writer now, without waiting for
    /// the timer and without blocking: leaving a note, losing focus, stopping
    /// a recording.
    pub fn flush_edits(&mut self, cx: &mut Context<Self>) {
        if self.autosave.flush() {
            self.write_workspace(false, cx);
        }
        self.drain_chat_history(cx);
    }

    /// Write everything and wait until it is on disk: quit, account switch,
    /// an explicit Save, sleep. Returns false if the workspace write failed.
    pub fn flush_all_blocking(&mut self, cx: &mut Context<Self>) -> bool {
        let saved = !self.autosave.flush() || self.write_workspace(true, cx);
        if let Some((path, data, generation)) = take_chat_history() {
            let ticket = begin();
            if let Err(error) = commit(&path, generation, ticket, |_| {
                crate::chat::write_history(&path, &data)
            }) {
                self.show_toast(&error, cx);
            }
        }
        wait_idle();
        saved
    }

    /// Serialise the workspace and write it, on this thread when `blocking`.
    /// A failure is shown exactly as a failed save always was.
    fn write_workspace(&mut self, blocking: bool, cx: &mut Context<Self>) -> bool {
        let job = crate::model::workspace_file().and_then(|path| {
            self.workspace
                .encode()
                .map(|data| (path, data, next_generation()))
        });
        let (path, data, generation) = match job {
            Ok(job) => job,
            Err(error) => {
                self.show_toast(&format!("Could not save: {error}"), cx);
                return false;
            }
        };
        let ticket = begin();
        if blocking {
            let result = commit(&path, generation, ticket, |first| {
                crate::model::write_workspace(&path, &data, first)
            });
            if let Err(error) = &result {
                self.show_toast(&format!("Could not save: {error}"), cx);
            }
            return result.is_ok();
        }
        // Hand the job to the background executor before returning. If it were
        // only queued behind this thread's next turn, a blocking flush right
        // after (`wait_idle` on the UI thread) would wait for a write that
        // could never start.
        let write = cx.background_executor().spawn(async move {
            commit(&path, generation, ticket, |first| {
                crate::model::write_workspace(&path, &data, first)
            })
        });
        cx.spawn(async move |this, cx| {
            if let Err(error) = write.await {
                let _ = this.update(cx, |this, cx| {
                    // Nothing reached the disk: keep the workspace dirty so
                    // the next edit or flush tries again.
                    this.autosave.retry();
                    this.show_toast(&format!("Could not save: {error}"), cx);
                });
            }
        })
        .detach();
        true
    }

    /// Start the background write for chat history queued by
    /// `save_chat_history`. Called from render, which always follows it.
    pub fn drain_chat_history(&mut self, cx: &mut Context<Self>) {
        let Some((path, data, generation)) = take_chat_history() else {
            return;
        };
        let ticket = begin();
        // As for the workspace: started now, awaited later.
        let write = cx.background_executor().spawn(async move {
            commit(&path, generation, ticket, |_| {
                crate::chat::write_history(&path, &data)
            })
        });
        cx.spawn(async move |this, cx| {
            if let Err(error) = write.await {
                let _ = this.update(cx, |this, cx| this.show_toast(&error, cx));
            }
        })
        .detach();
    }
}

/// Flush when the window stops being active (which covers hiding and closing
/// it) and before the system sleeps.
pub fn install(handle: WindowHandle<Cereal>, cx: &mut App) {
    let _ = handle.update(cx, |view, window, cx| {
        let subscription = cx.observe_window_activation(window, |view, window, cx| {
            if !window.is_window_active() {
                view.flush_edits(cx);
            }
        });
        view._subscriptions.push(subscription);
    });
    sleep::observe(handle, cx);
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod sleep {
    use super::*;
    use objc::{
        class,
        declare::ClassDecl,
        msg_send,
        runtime::{Object, Sel},
        sel, sel_impl,
    };
    use std::cell::RefCell;

    thread_local! {
        static FLUSH: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    }

    extern "C" fn will_sleep(_: &Object, _: Sel, _: *mut Object) {
        FLUSH.with(|flush| {
            if let Some(flush) = flush.borrow().as_ref() {
                flush();
            }
        });
    }

    /// `NSWorkspaceWillSleepNotification` arrives on the main thread. The
    /// handler only queues a foreground task, so it can never re-enter an
    /// update that is already running.
    pub fn observe(handle: WindowHandle<Cereal>, cx: &mut App) {
        let async_cx = cx.to_async();
        FLUSH.with(|flush| {
            *flush.borrow_mut() = Some(Box::new(move || {
                async_cx
                    .spawn(async move |cx| {
                        let _ = handle.update(cx, |view, _, cx| {
                            view.flush_all_blocking(cx);
                        });
                    })
                    .detach();
            }));
        });
        unsafe {
            let Some(mut declaration) = ClassDecl::new("CerealSleepObserver", class!(NSObject))
            else {
                return;
            };
            declaration.add_method(
                sel!(workspaceWillSleep:),
                will_sleep as extern "C" fn(&Object, Sel, *mut Object),
            );
            let observer: *mut Object = msg_send![declaration.register(), new];
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            let center: *mut Object = msg_send![workspace, notificationCenter];
            let name: *mut Object = msg_send![class!(NSString), stringWithUTF8String: c"NSWorkspaceWillSleepNotification".as_ptr()];
            let nil: *mut Object = std::ptr::null_mut();
            let _: () = msg_send![center, addObserver: observer selector: sel!(workspaceWillSleep:) name: name object: nil];
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod sleep {
    use super::*;
    pub fn observe(_: WindowHandle<Cereal>, _: &mut App) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;
    use std::time::Instant;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "cereal-store-{name}-{}-{}",
            std::process::id(),
            next_generation()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// Stand-in for the app's save path with a manual clock: edits mark the
    /// autosave, the timer fires [`SAVE_WINDOW`] after it was armed, and each
    /// write goes through [`commit`] like the real one.
    struct Editor {
        path: PathBuf,
        text: String,
        autosave: Autosave,
        timer: Option<Instant>,
        writes: usize,
    }

    impl Editor {
        fn new(path: PathBuf) -> Self {
            Self {
                path,
                text: String::new(),
                autosave: Autosave::default(),
                timer: None,
                writes: 0,
            }
        }

        fn write(&mut self) {
            let (generation, data) = (next_generation(), self.text.clone());
            let ticket = begin();
            let wrote = commit(&self.path, generation, ticket, |_| {
                std::fs::write(&self.path, &data).map_err(|error| error.to_string())
            })
            .unwrap();
            self.writes += usize::from(wrote);
        }

        fn key(&mut self, character: char, now: Instant) {
            self.advance(now);
            self.text.push(character);
            if self.autosave.touch() {
                self.timer = Some(now + SAVE_WINDOW);
            }
        }

        fn advance(&mut self, now: Instant) {
            if self.timer.is_some_and(|due| now >= due) {
                self.timer = None;
                if self.autosave.fire() {
                    self.write();
                }
            }
        }

        fn quit(&mut self) {
            if self.autosave.flush() {
                self.write();
            }
            wait_idle();
        }

        fn on_disk(&self) -> String {
            std::fs::read_to_string(&self.path).unwrap_or_default()
        }
    }

    #[test]
    fn rapid_edits_coalesce_into_one_write_with_the_final_content() {
        let directory = scratch("coalesce");
        let mut editor = Editor::new(directory.join("workspace.json"));
        let start = Instant::now();
        // Forty keys, 5 ms apart: all inside one save window.
        for (index, character) in "the quick brown fox jumps over the lazy d"
            .chars()
            .enumerate()
        {
            editor.key(character, start + Duration::from_millis(index as u64 * 5));
        }
        assert_eq!(editor.writes, 0, "nothing is written on the typing path");
        editor.advance(start + SAVE_WINDOW);
        assert_eq!(editor.writes, 1);
        assert_eq!(editor.on_disk(), editor.text);
        assert!(!editor.autosave.is_dirty());
        // Nothing changed since: the next timer, or a flush, writes nothing.
        editor.advance(start + SAVE_WINDOW * 4);
        editor.quit();
        assert_eq!(editor.writes, 1);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn continuous_typing_is_written_every_window_not_only_when_it_pauses() {
        let directory = scratch("throttle");
        let mut editor = Editor::new(directory.join("workspace.json"));
        let start = Instant::now();
        // Two seconds of typing without a single pause longer than 20 ms.
        for index in 0..100u64 {
            editor.key('x', start + Duration::from_millis(index * 20));
        }
        let windows = 2000 / SAVE_WINDOW.as_millis() as usize;
        assert!(
            editor.writes >= windows - 1 && editor.writes <= windows + 1,
            "{} writes",
            editor.writes
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn quitting_with_a_pending_edit_writes_it_before_exit() {
        let directory = scratch("quit");
        let mut editor = Editor::new(directory.join("workspace.json"));
        let start = Instant::now();
        editor.key('a', start);
        editor.key('b', start + Duration::from_millis(40));
        assert_eq!(editor.on_disk(), "", "still inside the save window");
        editor.quit();
        assert_eq!(editor.on_disk(), "ab");
        assert_eq!(editor.writes, 1);
        // The timer armed before the flush finds nothing left to do.
        editor.advance(start + SAVE_WINDOW * 2);
        assert_eq!(editor.writes, 1);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_crash_loses_at_most_the_save_window() {
        let directory = scratch("crash");
        let mut editor = Editor::new(directory.join("workspace.json"));
        let start = Instant::now();
        let mut typed_at = Vec::new();
        // A key every 10 ms; the process dies at an arbitrary moment with no
        // flush at all.
        for crash_after in [7u64, 31, 64, 123] {
            for index in typed_at.len() as u64..crash_after {
                let at = start + Duration::from_millis(index * 10);
                editor.key('k', at);
                typed_at.push(at);
            }
            let crashed = *typed_at.last().unwrap();
            let survived = editor.on_disk().len();
            for lost in &typed_at[survived..] {
                assert!(
                    crashed.duration_since(*lost) < SAVE_WINDOW,
                    "an edit older than the save window was not on disk"
                );
            }
        }
        assert!(SAVE_WINDOW <= Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_stale_write_never_lands_on_top_of_a_newer_one() {
        let directory = scratch("order");
        let path = directory.join("workspace.json");
        let write = |text: &'static str| {
            let path = path.clone();
            move |_: bool| std::fs::write(&path, text).map_err(|error| error.to_string())
        };
        let (older, newer) = (next_generation(), next_generation());
        // The newer content reaches the writer first (a slow background write
        // overtaken by a blocking flush).
        let ticket = begin();
        assert_eq!(commit(&path, newer, ticket, write("newer")), Ok(true));
        let ticket = begin();
        assert_eq!(commit(&path, older, ticket, write("older")), Ok(false));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer");
        wait_idle();
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_failed_write_is_reported_and_does_not_block_the_next_one() {
        let directory = scratch("failure");
        let path = directory.join("workspace.json");
        let ticket = begin();
        let failed = commit(&path, next_generation(), ticket, |_| {
            Err("disk is full".into())
        });
        assert_eq!(failed, Err("disk is full".to_string()));
        // The failure recorded nothing, so the file still counts as unknown
        // and the retry is told to validate what is there.
        let ticket = begin();
        let retried = commit(&path, next_generation(), ticket, |first| {
            assert!(first);
            std::fs::write(&path, "saved").map_err(|error| error.to_string())
        });
        assert_eq!(retried, Ok(true));
        let ticket = begin();
        let again = commit(&path, next_generation(), ticket, |first| {
            assert!(!first, "our own file is not re-validated on every save");
            Ok(())
        });
        assert_eq!(again, Ok(true));
        wait_idle();
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn readers_wait_for_writes_that_were_handed_over() {
        let directory = scratch("reader");
        let path = directory.join("workspace.json");
        let generation = next_generation();
        let ticket = begin();
        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                commit(&path, generation, ticket, |_| {
                    std::fs::write(&path, "handed over").map_err(|error| error.to_string())
                })
            })
        };
        // A reader arriving while the write is in flight sees it finished.
        wait_idle();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "handed over");
        assert_eq!(writer.join().unwrap(), Ok(true));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_blocking_flush_waits_only_for_writes_that_have_really_started() {
        // The rule the app follows: a write is counted (`begin`) only once
        // its job is running somewhere other than the waiting thread. Here
        // the "background executor" is a thread started before the wait.
        let directory = scratch("started");
        let path = directory.join("workspace.json");
        let generation = next_generation();
        let ticket = begin();
        let background = {
            let path = path.clone();
            std::thread::spawn(move || {
                commit(&path, generation, ticket, |_| {
                    std::fs::write(&path, "background").map_err(|error| error.to_string())
                })
            })
        };
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let waiter = {
            let finished = finished.clone();
            std::thread::spawn(move || {
                wait_idle();
                finished.store(true, Ordering::SeqCst);
            })
        };
        background.join().unwrap().unwrap();
        waiter.join().unwrap();
        assert!(finished.load(Ordering::SeqCst));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "background");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_write_that_is_discarded_unrun_does_not_strand_readers() {
        // The executor may drop a queued job (shutdown). Its ticket goes
        // with it, so nobody waits for a write that will never happen.
        let ticket = begin();
        let job = move || drop(ticket);
        let waiter = std::thread::spawn(wait_idle);
        std::thread::sleep(Duration::from_millis(20));
        drop(job);
        waiter.join().unwrap();
    }

    #[test]
    fn unchanged_chat_history_is_not_queued_again() {
        let path = PathBuf::from("/nonexistent/chat-history.json");
        // Tests share the process-wide slot; use content no other test does.
        let first = format!("history-{}", next_generation()).into_bytes();
        assert!(queue_chat_history(path.clone(), first.clone()));
        assert!(!queue_chat_history(path.clone(), first.clone()));
        let mut changed = first.clone();
        changed.push(b'!');
        assert!(queue_chat_history(path.clone(), changed.clone()));
        // Only the newest content is kept for the writer.
        let queued = take_chat_history().unwrap();
        assert_eq!((queued.0, queued.1), (path, changed));
    }
}
