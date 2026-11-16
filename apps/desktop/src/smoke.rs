//! Native event-loop smoke test, run only with --smoke-test and an isolated data directory.
use crate::{
    app::{Cereal, Screen},
    model::Workspace,
};
use gpui::*;

pub fn run(handle: WindowHandle<Cereal>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut id=0;
        for step in 0..59 {
            let delay=match step {13=>850,15|17=>650,16|47=>2200,19=>400,20=>1900,31|33=>950,35=>750,36|43=>250,48|52=>650,50=>750,51=>1000,55=>1900,24|26=>450,_=>180};
            cx.background_executor().timer(std::time::Duration::from_millis(delay)).await;
            if step==35 {
                cx.update(|cx| {
                    assert_eq!(crate::pill::recording_window_size(cx),Some(size(px(42.),px(70.))),"Background recording creates the separate native Nub");
                    crate::pill::smoke_pointer_move(point(px(21.),px(34.)),cx);
                    assert!(crate::pill::transcript_window_size(cx).is_none(),"Hover respects its opening delay");
                    crate::pill::smoke_pointer_event(false,PlatformInput::MouseExited(MouseExitEvent::default()),cx);
                }).unwrap();
            }
            if step==36 {
                cx.update(|cx| {
                    assert!(crate::pill::transcript_window_size(cx).is_none(),"Leaving before the hover delay cancels opening");
                    crate::pill::smoke_pointer_move(point(px(21.),px(34.)),cx);
                }).unwrap();
            }
            if step==37 {
                cx.update(|cx| {
                    assert_eq!(crate::pill::transcript_window_size(cx),Some(size(px(300.),px(350.))),"A button-free pointer entry opens the native hover transcript");
                    crate::pill::smoke_pointer_event(false,PlatformInput::MouseExited(MouseExitEvent::default()),cx);
                    crate::pill::smoke_pointer_event(true,PlatformInput::MouseMove(MouseMoveEvent{position:point(px(150.),px(180.)),..Default::default()}),cx);
                }).unwrap();
            }
            if step==38 {
                cx.update(|cx| {
                    assert!(crate::pill::transcript_window_size(cx).is_some(),"Moving from pill to transcript keeps the panel open");
                    crate::pill::smoke_pointer_event(true,PlatformInput::MouseExited(MouseExitEvent::default()),cx);
                }).unwrap();
            }
            if step==39 {
                cx.update(|cx| assert!(crate::pill::transcript_window_size(cx).is_none(),"Leaving both native windows dismisses the transcript")).unwrap();
            }
            if step==41 {
                cx.update(|cx| assert!(crate::pill::recording_window_size(cx).is_none(),"Stopping removes the desktop recording window")).unwrap();
            }
            if step==43 {
                cx.update(|cx| assert_eq!(crate::pill::recording_window_size(cx),Some(size(px(42.),px(70.))),"Closing the main window leaves the desktop recording pill")).unwrap();
            }
            handle.update(cx,|view,window,cx| {
                match step {
                    0=>{
                        view.new_note(window,cx);id=view.current_note.unwrap();
                        view.title_input.update(cx,|input,cx|input.replace_text_in_range(None,"Native test – café 🦀",window,cx));
                        view.editor.update(cx,|input,cx|input.replace_text_in_range(None,"Notes from the review\n\nTodo: Maya to share the prototype\nKeep onboarding simple.",window,cx));
                    },
                    1=>{
                        let note=view.workspace.note(id).unwrap();
                        assert_eq!(note.title,"Native test – café 🦀");
                        assert!(note.body.contains("Maya"));
                        view.navigate(Screen::Home,cx);
                        assert!(view.workspace.note(id).unwrap().enhanced.is_empty(),"Navigation does not generate notes");
                        view.open_search(window,cx);
                        view.search.update(cx,|input,cx|input.set_content("Native test",cx));
                    },
                    2=>{view.search.update(cx,|input,cx|cx.emit(crate::input::InputSubmitted{content:input.content()}));},
                    3=>{
                        assert_eq!(view.screen,Screen::SearchResults,"A nonempty search submission opens its complete results page");
                        assert_eq!(view.current_note,None,"Searching does not open or edit the first matching note");
                        assert_eq!(view.search_results_query,"Native test");
                        assert!(view.workspace.visible_notes(false,&view.search_results_query).iter().any(|note|note.id==id),"The results page uses the same real note search as the palette");
                        assert!(!view.search_open);
                        view.open_note(id,cx);
                        view.navigate(Screen::MyNotes,cx);
                        view.ask.update(cx,|input,cx|input.set_content("What did we decide about onboarding?",cx));
                        view.submit_chat(cx);
                        assert!(view.chat_messages.last().unwrap().1.contains("onboarding"));
                        let original_chat=view.chat_messages.clone();
                        view.ask.update(cx,|input,cx|input.set_content("Unsent café draft",cx));
                        view.begin_chat_edit(0,window,cx);
                        assert_eq!(view.chat_messages,original_chat,"Editing does not discard the original thread");
                        view.ask.update(cx,|input,cx|input.set_content("A replacement not sent",cx));
                        view.cancel_chat_edit(cx);
                        assert_eq!(view.chat_messages,original_chat,"Cancel preserves all original turns");
                        assert_eq!(view.ask.read(cx).content(),"Unsent café draft","Cancel restores the previous composer draft");
                        view.begin_chat_edit(0,window,cx);
                        view.ask.update(cx,|input,cx|input.set_content("What about onboarding next steps?",cx));
                        view.submit_chat(cx);
                        assert_eq!(view.chat_messages.len(),1,"Sending an edit replaces its old turn rather than appending a duplicate");
                        assert_eq!(view.chat_messages[0].0,"What about onboarding next steps?");
                        assert!(view.chat_editing.is_none());
                        view.ask.update(cx,|input,cx|input.set_content("/loop",cx));
                        view.submit_chat(cx);
                        assert_eq!(view.chat_messages.last().unwrap().0,"/loop","A question starting with a slash is sent as it was typed");
                    },
                    4=>{
                        view.workspace.note_mut(id).unwrap().trashed=true;
                        assert!(view.workspace.visible_notes(false,"Native test").is_empty());
                        view.open_trash(cx);
                    },
                    5=>{
                        view.workspace.note_mut(id).unwrap().trashed=false;
                        assert_eq!(view.workspace.visible_notes(false,"Native test").len(),1);
                        view.workspace.settings.force_dark(true);view.navigate(Screen::Settings,cx);
                    },
                    6=>{view.navigate(Screen::Chat,cx);},
                    7=>{
                        assert!(view.persist(cx));
                        view.flush_all_blocking(cx);let reloaded=Workspace::load();
                        assert_eq!(reloaded.note(id).unwrap().title,"Native test – café 🦀");
                        assert!(reloaded.settings.dark_mode);
                        view.open_note(1,cx);
                        let original_summary=view.workspace.note(1).unwrap().enhanced.clone();
                        view.active_tab=crate::app::NoteTab::Notes;
                        view.finish_note(cx);
                        assert_eq!(view.workspace.note(1).unwrap().enhanced,original_summary,"Viewing My notes must not overwrite an existing summary");
                        view.open_note(id,cx);view.assistant_open=true;
                    },
                    8=>{view.navigate(Screen::Login,cx);},
                    9=>{
                        view.login.update(cx,|login,cx|login.start_provider("Google",cx));
                        assert!(matches!(view.login.read(cx).stage,crate::login::LoginStage::Waiting{..}));
                    },
                    10=>{view.login.update(cx,|login,cx|{login.reset(cx);login.reveal_sso(cx);});},
                    11=>{
                        view.login.update(cx,|login,cx|{
                            login.email.update(cx,|input,cx|input.set_content("preview@example.invalid",cx));
                            login.submit_email(cx);
                        });
                        assert!(matches!(view.login.read(cx).stage,crate::login::LoginStage::VerifyEmail{..}));
                    },
                    12=>{
                        view.login.update(cx,|login,cx|{
                            login.code.update(cx,|input,cx|input.set_content("local-preview",cx));
                            login.verify_preview(cx);
                        });
                    },
                    13=>{assert_eq!(view.screen,Screen::Home,"Sign-in preview completion returns Home");},
                    14=>{
                        view.open_note(id,cx);view.assistant_open=false;
                        view.start_recording_preview(id,cx);
                        assert_eq!(view.recording.phase,crate::recording::Phase::Starting);
                        view.stop_recording_preview(cx);
                        assert_eq!(view.recording.phase,crate::recording::Phase::Starting,"Rapid duplicate clicks are guarded");
                    },
                    15=>{
                        assert_eq!(view.recording.phase,crate::recording::Phase::Live);
                        assert!(view.recording.pause());cx.notify();
                    },
                    16=>{
                        assert_eq!(view.recording.phase,crate::recording::Phase::Held);
                        view.start_recording_preview(id,cx);
                        assert_eq!(view.recording.phase,crate::recording::Phase::Starting);
                    },
                    17=>{
                        assert_eq!(view.recording.phase,crate::recording::Phase::Live);
                        view.transcript_open=true;view.transcript_epoch=view.transcript_epoch.wrapping_add(1);
                        view.menu=Some(crate::app::Menu::RecordingSettings);cx.notify();
                    },
                    18=>{
                        view.stop_recording_preview(cx);
                        assert_eq!(view.recording.phase,crate::recording::Phase::Complete);
                        assert!(view.workspace.note(id).unwrap().enhanced.is_empty(),"Stop only stops capture");
                        assert!(!view.transcript_open);
                    },
                    19=>{
                        assert_eq!(view.generation.phase,crate::recording::GenerationPhase::Idle);
                        view.enhance_note_preview(id,cx);
                        assert_eq!(view.generation.phase,crate::recording::GenerationPhase::AnalyzingTranscript);
                    },
                    20=>{
                        assert_eq!(view.recording.phase,crate::recording::Phase::Complete);
                        assert!(view.workspace.note(id).unwrap().enhanced.contains("Maya"));
                    },
                    21=>{assert!(view.recording.begin(id));assert!(view.recording.fail(view.recording.epoch));cx.notify();},
                    22=>{assert_eq!(view.recording.phase,crate::recording::Phase::Failed);view.recording.reset();},
                    23=>{
                        view.menu=Some(crate::app::Menu::NoteDetails);
                        view.attendee_input.update(cx,|input,cx|input.set_content("Maya Example",cx));
                        view.attendee_input.read(cx).focus_handle.focus(window);cx.notify();
                    },
                    24=>{view.attendee_input.update(cx,|input,cx|cx.emit(crate::input::InputSubmitted{content:input.content()}));},
                    25=>{
                        assert!(view.workspace.note(id).unwrap().participants.iter().any(|name|name=="Maya Example"));
                        view.workspace.note_mut(1).unwrap().date=chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
                        view.search.update(cx,|input,cx|input.set_content("Product",cx));
                        view.menu=Some(crate::app::Menu::CalendarEvent);cx.notify();
                    },
                    26=>{view.search.update(cx,|input,cx|cx.emit(crate::input::InputSubmitted{content:input.content()}));},
                    27=>{assert_eq!(view.menu,Some(crate::app::Menu::NoteDetails));assert_eq!(view.workspace.note(id).unwrap().time,view.workspace.note(1).unwrap().time);},
                    28=>{
                        view.menu=None;
                        assert!(view.recording.begin(id));
                        assert!(view.recording.activate(view.recording.epoch));
                        assert!(view.recording.stop());
                        let old_epoch=view.generation.begin(id,view.workspace.note(id).unwrap().transcript_revision);
                        view.new_note(window,cx);
                        assert_ne!(view.current_note,Some(id));
                        assert_eq!(view.recording.note_id,view.current_note);
                        assert_eq!(view.recording.phase,crate::recording::Phase::Starting,"New note starts immediately after Stop");
                        assert!(!view.generation.writing(old_epoch),"Old generation cannot change the new session");
                        assert!(!view.workspace.note(id).unwrap().duration.is_empty());
                        view.finish_recording_preview(cx);
                        assert!(view.persist(cx));
                    },
                    29=>{
                        view.open_note(id,cx);
                        view.active_tab=crate::app::NoteTab::Enhanced;
                        view.workspace.note_mut(id).unwrap().enhanced="## Summary\n\n- Share the onboarding prototype".into();
                        view.workspace.note_mut(id).unwrap().transcript="Maya  01:20\nMaya will share the onboarding prototype after this review.".into();
                        view.open_summary(id,2,"Share the onboarding prototype".into(),Bounds::new(point(px(900.),px(180.)),size(px(24.),px(24.))),cx);
                        assert!(!view.summary.open.as_ref().unwrap().ready);
                    },
                    30=>{assert!(!view.summary.open.as_ref().unwrap().ready,"Summary skeleton remains visible before local loading completes");},
                    31=>{
                        let summary=view.summary.open.as_ref().unwrap();
                        assert!(summary.ready);
                        assert_eq!(summary.quotes[0].speaker.as_deref(),Some("Maya"));
                        assert!(summary.quotes[0].text.contains("prototype"));
                        view.open_search(window,cx);
                        assert!(view.summary.open.is_none(),"Search supersedes an open summary card");
                        view.search_open=false;
                    },
                    32=>{
                        view.summary.dismiss();
                        view.open_summary(id,2,"Share the onboarding prototype".into(),Bounds::new(point(px(900.),px(180.)),size(px(24.),px(24.))),cx);
                        view.navigate(Screen::Home,cx);
                    },
                    33=>{assert!(view.summary.open.is_none(),"Leaving a note invalidates its loading summary");},
                    34=>{
                        view.new_note(window,cx);
                        assert!(view.recording.set_transcription_attention(crate::recording::TranscriptionAttention::Warning));
                        cx.notify();
                        window.minimize_window();
                    },
                    35..=38=>{
                        assert!(view.recording.is_active(),"A transcription warning retains the active session and desktop pill");
                        assert_eq!(view.recording.transcription_attention,crate::recording::TranscriptionAttention::Warning);
                    },
                    39=>{
                        crate::pill::reveal_main_window(window);
                        window.activate_window();
                    },
                    40=>{
                        view.finish_recording_preview(cx);
                        assert_eq!(view.recording.transcription_attention,crate::recording::TranscriptionAttention::None,"Stopping clears the warning with the recording owner");
                    },
                    41=>{},
                    42=>{
                        view.new_note(window,cx);id=view.current_note.unwrap();
                        view.title_input.update(cx,|input,cx|input.replace_text_in_range(None,"Retained background session",window,cx));
                        crate::pill::hide_main_window(window,cx);
                    },
                    43=>{
                        assert!(view.recording.is_active(),"Closing must preserve the recording");
                        assert_eq!(view.recording.note_id,view.current_note);
                        assert_eq!(view.workspace.note(view.current_note.unwrap()).unwrap().title,"Retained background session");
                        crate::pill::reveal_main_window(window);
                        window.activate_window();
                    },
                    44=>{view.finish_recording_preview(cx);},
                    45=>{assert!(view.persist(cx));},
                    46=>{
                        view.workspace.note_mut(id).unwrap().replace_transcript("Maya  00:00\nShare the onboarding prototype with five reviewers.");
                        cx.notify();
                    },
                    47=>{view.start_recording_preview(id,cx);},
                    48=>{
                        assert!(view.recording.is_active());
                        view.stop_recording_preview(cx);
                        assert_eq!(view.recording.phase,crate::recording::Phase::Complete);
                        assert_eq!(view.generation.phase,crate::recording::GenerationPhase::Idle);
                        assert!(view.workspace.note(id).unwrap().enhanced.is_empty());
                    },
                    49=>{
                        view.enhance_note_preview(id,cx);
                        assert_eq!(view.generation.phase,crate::recording::GenerationPhase::AnalyzingTranscript);
                    },
                    50=>{assert_eq!(view.generation.phase,crate::recording::GenerationPhase::WritingNotes);},
                    51=>{
                        assert_eq!(view.generation.phase,crate::recording::GenerationPhase::Ready);
                        let note=view.workspace.note(id).unwrap();
                        assert_eq!(note.generated_revision,Some(note.transcript_revision));
                        assert!(note.enhanced.contains("onboarding"));
                        let previous_transcript=note.transcript.clone();
                        let previous_elapsed=view.recording.elapsed();
                        view.start_recording_preview(id,cx);
                        assert_eq!(view.recording.elapsed(),previous_elapsed,"Resume preserves the elapsed duration");
                        assert_eq!(view.workspace.note(id).unwrap().transcript,previous_transcript,"Resume preserves the existing transcript");
                    },
                    52=>{
                        assert!(view.recording.is_active());
                        view.workspace.note_mut(id).unwrap().append_transcript("Theo  00:22\nWe will review the feedback next Monday.");
                        let old_outline=view.workspace.note(id).unwrap().enhanced.clone();
                        view.stop_recording_preview(cx);
                        assert!(view.workspace.note(id).unwrap().needs_regeneration());
                        assert_eq!(view.workspace.note(id).unwrap().enhanced,old_outline,"New transcript content does not silently overwrite the generated note");
                    },
                    53=>{
                        view.enhance_note_preview(id,cx);
                        let cancelled=view.generation.epoch;
                        view.navigate(Screen::Home,cx);
                        assert!(!view.generation.writing(cancelled),"Navigation invalidates pending generation");
                    },
                    54=>{view.open_note(id,cx);view.enhance_note_preview(id,cx);},
                    55=>{
                        let note=view.workspace.note(id).unwrap();
                        assert_eq!(view.generation.phase,crate::recording::GenerationPhase::Ready);
                        assert_eq!(note.generated_revision,Some(note.transcript_revision));
                        assert!(!note.needs_regeneration());
                        assert!(note.enhanced.contains("Monday"));
                    },
                    56=>{
                        view.enhance_note_preview(id,cx);
                        let cancelled=view.generation.epoch;
                        view.start_recording_preview(id,cx);
                        assert!(!view.generation.writing(cancelled),"Resume invalidates pending generation");
                        view.finish_recording_preview(cx);
                    },
                    57=>{
                        view.open_note(id,cx);
                        view.clear_chat(cx);
                        view.ask.update(cx,|input,cx|input.set_content("Which onboarding next steps belong to this note?",cx));
                        view.submit_chat(cx);
                        let note_thread=view.chat_thread_id.clone();
                        let note_messages=view.chat_messages.clone();
                        assert_eq!(view.current_note,Some(id));
                        // Header Back and Escape use this same production exit.
                        view.navigate(Screen::Home,cx);
                        assert_eq!(view.current_note,None);
                        assert_ne!(view.chat_thread_id,note_thread,"Leaving a note restores a distinct workspace conversation");
                        assert!(!view.chat_messages.iter().any(|turn|turn.0=="Which onboarding next steps belong to this note?"),"A note's turns must not become workspace history");
                        view.navigate(Screen::Chat,cx);
                        assert_eq!(view.chat_archives.iter().find(|chat|chat.id==note_thread).unwrap().note_id,id,"Opening workspace Chat cannot reclassify an archived note thread");
                        // History and workspace Recents invoke this same method.
                        view.restore_archived_chat(&note_thread,true,cx);
                        assert_eq!(view.current_note,Some(id),"Restoring a note thread restores the real request's note scope");
                        assert_eq!(view.chat_thread_id,note_thread);
                        assert_eq!(view.chat_messages,note_messages);
                        assert!(view.chat_maximized && view.assistant_open);
                        assert!(view.persist(cx));
                        view.flush_all_blocking(cx);let loaded=Workspace::load();
                        let note=loaded.note(id).unwrap();
                        assert_eq!(note.generated_revision,Some(note.transcript_revision));
                        assert!(!note.needs_regeneration());
                    },
                    _=>{
                        println!("Native smoke test passed: input events, Unicode editing, autosave, local outline, search submission, chat, trash/restore, settings, reload, sign-in preview, recording start/pause/resume/stop without automatic generation, explicit analyze/write generation, transcript revision/regeneration, stale generation guards, transcript-summary loading/quotes/navigation cancellation, and rendering.");
                        cx.quit();
                    }
                }
            }).expect("Native smoke window remains open");
        }
    }).detach();
}
