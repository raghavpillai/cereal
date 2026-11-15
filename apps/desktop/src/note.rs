use crate::{
    app::{Cereal, Menu, NoteTab},
    model::Note,
    ui::*,
};
use chrono::{Datelike, Local, NaiveDate};
use gpui::{prelude::*, *};

/// Stable local-day labels are shared by the chip and its anchored popovers.
pub(crate) fn note_date_label(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        "Today".into()
    } else if Some(date) == today.pred_opt() {
        "Yesterday".into()
    } else {
        date.format("%b %-d").to_string()
    }
}

fn metadata_labels(note: &Note, today: NaiveDate) -> (String, String) {
    let date = note
        .meeting_context
        .as_ref()
        .and_then(crate::meeting::meeting_date)
        .or_else(|| NaiveDate::parse_from_str(&note.date, "%Y-%m-%d").ok())
        .unwrap_or(today);
    let people = note
        .meeting_context
        .as_ref()
        .map(|meeting| crate::meeting::participants(meeting).len())
        .filter(|count| *count > 0)
        .map(|count| count.to_string())
        .unwrap_or_else(|| "Me".into());
    (note_date_label(date, today), people)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NoteMetadataAnchors {
    pub details_left: f32,
    pub folder_left: f32,
}

/// The document is centered independently of sidebar state on a note page.
/// Use the same content gutter and joined-tab sizes as the actual metadata row.
fn metadata_anchors(
    viewport_width: f32,
    has_enhanced: bool,
    active: NoteTab,
    metadata_width: f32,
) -> NoteMetadataAnchors {
    let document_width = viewport_width.min(if has_enhanced { 682. } else { 672. });
    let gutter_left = (viewport_width - document_width) / 2. + 29.;
    let joined_views = if !has_enhanced {
        0.
    } else if active == NoteTab::Enhanced {
        148.
    } else {
        // 100 px "My notes" + the 46 px collapsed Enhanced tab + the border.
        148.
    };
    let details_left = gutter_left + if has_enhanced { joined_views + 8. } else { 0. };
    let clamp = |left: f32, width: f32| left.clamp(12., (viewport_width - width - 12.).max(12.));
    NoteMetadataAnchors {
        details_left: clamp(details_left, 280.),
        folder_left: clamp(details_left + metadata_width + 8., 288.),
    }
}

impl Cereal {
    pub(crate) fn note_metadata_anchors(&self, window: &Window) -> NoteMetadataAnchors {
        let Some(note) = self.current_note.and_then(|id| self.workspace.note(id)) else {
            return metadata_anchors(
                f32::from(window.viewport_size().width),
                false,
                NoteTab::Notes,
                128.,
            );
        };
        let enhanced = !note.enhanced.trim().is_empty() || self.generation.is_running_for(note.id);
        let (date, people) = metadata_labels(note, Local::now().date_naive());
        let text_width = |label: &str| {
            let text: SharedString = label.to_owned().into();
            let runs = [TextRun {
                len: text.len(),
                font: font(FONT),
                color: self.theme().text,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            f32::from(
                window
                    .text_system()
                    .shape_line(text, px(13.), &runs, None)
                    .width,
            )
        };
        // Two14px icons, three6px gaps, horizontal12px padding and1px borders.
        let width = 72. + text_width(&date) + text_width(&people);
        metadata_anchors(
            f32::from(window.viewport_size().width),
            enhanced,
            self.active_tab,
            width,
        )
    }

    pub fn render_note(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        // Not a clone: only what this page reads (see `Note::frame_copy`).
        let Some(note) = self
            .current_note
            .and_then(|id| self.workspace.note(id))
            .map(crate::model::Note::frame_copy)
        else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(small("This note is no longer available.", t))
                .into_any_element();
        };
        crate::motion::stats::copied(note.bulk());
        let Some(note) = Some(note) else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(small("This note is no longer available.", t))
                .into_any_element();
        };
        let id = note.id;
        let generating = self.generation.is_running_for(id);
        let has_enhanced = !note.enhanced.trim().is_empty() || generating;
        let document_width =
            window
                .viewport_size()
                .width
                .min(px(if has_enhanced { 682. } else { 672. }));
        let body_width = (document_width - px(64.)).max(px(100.));
        let display_tab = if has_enhanced && self.active_tab == NoteTab::Enhanced {
            NoteTab::Enhanced
        } else {
            NoteTab::Notes
        };
        // The active tab's fill is one shape that flows across to the other
        // tab while both tabs trade widths on the same spring.
        let tabs = self.stage.glide("note-tabs");
        let mut views = div()
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(0.))
            .rounded_full()
            .border_1()
            .border_color(t.hairline)
            // A sunken track; the selected tab is a raised, outlined capsule.
            .bg(t.sidebar)
            .flex_shrink_0()
            .child(tabs.outlined_indicator(t.card, Some(t.hairline), 14.));
        for (index, (tab, label, symbol)) in [
            (NoteTab::Notes, "My notes", "notes-lines"),
            (NoteTab::Enhanced, "Enhanced", "sparkles"),
        ]
        .into_iter()
        .enumerate()
        {
            let active = display_tab == tab;
            let enhanced = tab == NoteTab::Enhanced;
            let target = if active {
                if enhanced { 112. } else { 100. }
            } else if enhanced {
                // Collapsed, Enhanced still shows its menu chevron.
                46.
            } else {
                34.
            };
            let width = self
                .stage
                .aim(("note-tab", index as u64), target, crate::motion::GOOEY);
            // Content is pinned to the leading edge with the inset that centres
            // it at rest, so the icon stays put and the label is uncovered as
            // the tab widens instead of sliding in from a clipped centre.
            let content = if active {
                let runs = [TextRun {
                    len: label.len(),
                    font: font(FONT),
                    color: t.text,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }];
                let text = f32::from(
                    window
                        .text_system()
                        .shape_line(label.into(), px(13.), &runs, None)
                        .width,
                );
                15. + 6. + text + if enhanced { 12. } else { 0. }
            } else if enhanced {
                15. + 12.
            } else {
                15.
            };
            let inset = self.stage.aim(
                ("note-tab-inset", index as u64),
                ((target - content) / 2.).max(0.),
                crate::motion::GOOEY,
            );
            views = views.child(
                round_tool(
                    SharedString::from(format!("note-tab-{label}")),
                    if enhanced && active {
                        "sparkles-solid"
                    } else {
                        symbol
                    },
                    label,
                    t,
                )
                .w(px(width.max(28.)))
                .h(px(28.))
                .justify_start()
                .pl(px(inset.max(0.)))
                .gap(px(6.))
                .text_size(px(13.))
                .whitespace_nowrap()
                .overflow_hidden()
                .when(active, |d| {
                    d.text_color(t.text).child(tabs.probe(index)).child(label)
                })
                // The chevron hugs the label, and stays on the collapsed tab.
                .when(enhanced, |d| {
                    d.child(icon("chevron-down", 9., t.secondary).ml(px(-3.)))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus.focus(window);
                    if active && tab == NoteTab::Enhanced {
                        this.menu = Some(Menu::Templates);
                    } else {
                        this.summary.dismiss();
                        this.active_tab = tab;
                    }
                    cx.notify();
                })),
            );
        }
        // The generation chip has a presence of its own. If the user watched
        // the generation finish, the chip leaves and the notes arrive block
        // by block, once. If it finished while they were elsewhere, the
        // notes are simply there when they come back.
        let watched = self.stage.continuous(("note-view", id));
        let was_generating = self.stage.target(("generation", id)) == Some(1.);
        if !watched && !generating {
            self.stage.forget(("generation", id));
        }
        let chip = self
            .stage
            .presence(("generation", id), generating, crate::motion::GOOEY);
        if watched
            && was_generating
            && !generating
            && self.generation.phase == crate::recording::GenerationPhase::Ready
            && display_tab == NoteTab::Enhanced
        {
            self.stage.mark("enhanced");
        }
        let content = match display_tab {
            NoteTab::Notes=>div().w(body_width).min_h(px(300.)).text_size(px(16.)).line_height(px(24.)).child(self.editor.clone()).into_any_element(),
            NoteTab::Enhanced if generating=>div().w(body_width).flex().flex_col().gap(px(12.))
                .child(div().text_size(px(16.)).line_height(px(24.)).text_color(t.muted).child("Add your thoughts…"))
                .child(self.render_generation_status(body_width + px(48.), chip))
                .when(!note.enhanced.trim().is_empty(), |d| d.child(self.render_enhanced_content(&note,t,body_width,window,cx)))
                .into_any_element(),
            // Generation just ended: the chip collapses into its leading edge
            // and its 88 px (placeholder, chip and their gaps) close up with
            // it, so the notes slide into place instead of jumping.
            NoteTab::Enhanced if chip.visible=>div().w(body_width).flex().flex_col()
                // Clipped to the closing block (which is widened to the chip's
                // 24 px overhang), so the chip is covered as the notes rise
                // rather than drawn across their first heading.
                .child(div().h(px(88. * chip.value.clamp(0., 1.))).mx(px(-24.)).px(px(24.)).overflow_hidden().flex().flex_col().gap(px(12.))
                    .child(div().text_size(px(16.)).line_height(px(24.)).text_color(t.muted).opacity(chip.fade).child("Add your thoughts…"))
                    .child(self.render_generation_status(body_width + px(48.), chip)))
                .when(!note.enhanced.trim().is_empty(), |d| d.child(self.render_enhanced_content(&note,t,body_width,window,cx)))
                .into_any_element(),
            NoteTab::Enhanced if note.enhanced.trim().is_empty()=>div().w(body_width).flex().flex_col().items_center().py(px(48.)).gap(px(12.))
                .child(icon("sparkles",24.,t.accent)).child(heading("A little more clarity",t))
                .child(div().w(px(340.)).max_w_full().text_center().text_size(px(14.)).line_height(px(21.)).text_color(t.muted)
                    .child("Write your notes, then make a local outline to put your thoughts in order."))
                .child(pill("enhance-local","Enhance notes","sparkles",t).on_click(cx.listener(move|this,_,_,cx|this.enhance_note_preview(id,cx)))).into_any_element(),
            NoteTab::Enhanced=>self.render_enhanced_content(&note,t,body_width,window,cx).into_any_element(),
        };
        let parsed_date = note
            .meeting_context
            .as_ref()
            .and_then(crate::meeting::meeting_date)
            .unwrap_or_else(|| {
                NaiveDate::parse_from_str(&note.date, "%Y-%m-%d")
                    .unwrap_or_else(|_| Local::now().date_naive())
            });
        let (date_label, people_label) = metadata_labels(&note, Local::now().date_naive());
        let month = parsed_date.with_day(1).unwrap_or(parsed_date);
        let document = div()
            .flex()
            .flex_col()
            .w(document_width)
            .min_w_0()
            .max_w(px(682.))
            .mx_auto()
            .px(px(32.))
            .pt(px(8.))
            .pb(px(136.))
            .child(
                div()
                    .w(body_width)
                    .font_family(DISPLAY)
                    .text_size(px(24.))
                    .line_height(px(28.))
                    .mb(px(14.))
                    .child(self.title_input.clone()),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .ml(px(-4.))
                    .mb(px(20.))
                    .when(has_enhanced, |d| d.child(views))
                    .child(
                        div()
                            .id("note-details")
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .h(px(28.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.hairline)
                            .text_size(px(13.))
                            .text_color(t.muted)
                            .cursor_pointer()
                            .child(row_feedback(t, 0.).round())
                            .child(icon("calendar-dots", 14., t.muted))
                            .child(date_label)
                            .child(icon("people", 14., t.muted).ml(px(4.)))
                            .child(people_label)
                            .reach().on_click(cx.listener(move |this, _, window, cx| {
                                this.focus.focus(window);
                                this.note_calendar_month = month;
                                if crate::server::enabled() {
                                    this.refresh_meeting_context(id, cx);
                                }
                                this.menu = Some(Menu::NoteDetails);
                                cx.notify();
                            })),
                    )
                    .child(if has_enhanced {
                        let folder = note.folder_id.and_then(|folder| {
                            self.workspace.folders.iter().find(|f| f.id == folder)
                        });
                        let filed = folder.is_some();
                        let label = folder
                            .map(|folder| folder.name.clone())
                            .unwrap_or_else(|| "Add to folder".into());
                        // The chip sizes to its label; only a very long folder
                        // name is shortened, and then with an ellipsis.
                        div()
                            .id("note-add-folder")
                            .h(px(28.))
                            .max_w(px(220.))
                            .min_w_0()
                            .px(px(11.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.hairline)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .text_color(t.muted)
                            .cursor_pointer()
                            .child(row_feedback(t, 0.).round())
                            .child(icon(
                                if filed { "folder" } else { "folder-plus" },
                                14.,
                                t.muted,
                            ))
                            .child(div().flex_1().min_w_0().truncate().child(label))
                            .reach().on_click(cx.listener(|this, _, window, cx| {
                                this.focus.focus(window);
                                this.search.update(cx, |input, cx| {
                                    input.set_content("", cx);
                                    input.set_placeholder("Search", cx);
                                });
                                this.menu = Some(Menu::MoveTo);
                                this.search.read(cx).focus_handle.focus(window);
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        round_tool("note-add-folder", "folder-plus", "Add to folder", t)
                            .size(px(28.))
                            .border_1()
                            .border_color(t.hairline)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.focus.focus(window);
                                this.search.update(cx, |input, cx| {
                                    input.set_content("", cx);
                                    input.set_placeholder("Search", cx);
                                });
                                this.menu = Some(Menu::MoveTo);
                                this.search.read(cx).focus_handle.focus(window);
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            )
            // Switching between My notes and Enhanced settles in rather than
            // swapping abruptly. Keyed by the tab only, so typing never replays it.
            .child(crate::motion::crossfade(
                ("note-tab-content", display_tab as u64),
                div().child(content),
            ));
        let transcript = self.stage.presence(
            ("transcript", 0),
            self.transcript_open,
            crate::motion::PANEL,
        );
        let assistant =
            self.stage
                .presence(("assistant", 0), self.assistant_open, crate::motion::PANEL);
        let owned = self.recording.note_id == Some(id);
        let live = owned && self.recording.is_active();
        let suggestion = if live {
            "What did I miss"
        } else {
            "Write follow up email"
        };
        let footer = div()
            .absolute()
            .bottom(px(26.))
            .left(px(24.))
            .right(px(24.))
            .flex()
            .justify_center()
            .child(bar_scrim(t, 24.))
            .child(
                div()
                    .w_full()
                    .max_w(px(672.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.render_recording_control(id, cx))
                    .when(!assistant.visible, |d| {
                        d.child(
                            div()
                                .h(px(59.))
                                .flex_1()
                                .min_w_0()
                                .rounded_full()
                                .border_1()
                                .border_color(t.border)
                                .bg(t.card)
                                .shadow(vec![BoxShadow {
                                    color: rgba(0x29292908).into(),
                                    offset: point(px(0.), px(2.)),
                                    blur_radius: px(5.),
                                    spread_radius: px(0.),
                                }])
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .pl(px(22.))
                                .pr(px(11.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(14.))
                                        .line_height(px(22.))
                                        .child(self.ask.clone()),
                                )
                                .when(document_width >= px(560.), |d| {
                                    d.child(
                                        pill("note-follow-up", suggestion, "command", t)
                                            .h(px(36.))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.ask.update(cx, |input, cx| {
                                                    input.set_content(
                                                        if live {
                                                            "What did I miss?"
                                                        } else {
                                                            "Write follow up email"
                                                        },
                                                        cx,
                                                    )
                                                });
                                                this.submit_chat(cx);
                                            })),
                                    )
                                }),
                        )
                    }),
            );
        let mut result = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("note-document-scroll")
                            .track_scroll(&self.note_scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(document),
                    )
                    .child(crate::ui::scroll_thumb("note-document", &self.note_scroll))
                    // The transcript unfolds out of the footer bar and folds back
                    // into it; the bar only returns once the panel has landed.
                    .when(!transcript.visible, |d| d.child(footer))
                    .when(transcript.visible, |d| {
                        d.child(
                            self.render_transcript_panel(
                                id,
                                (document_width + px(8.)).min(px(682.)),
                                (window.viewport_size().height - px(100.))
                                    .min(px(465.))
                                    .max(px(280.)),
                                transcript,
                                window,
                                cx,
                            ),
                        )
                    })
                    .when(
                        owned && self.audio_error.is_some() && !crate::audio::preview_mode(),
                        |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .bottom(px(78.))
                                    .left(px(24.))
                                    .right(px(24.))
                                    .text_center()
                                    .text_size(px(11.))
                                    .line_height(px(15.))
                                    .text_color(rgb(0xc08038))
                                    .child(self.audio_error.clone().unwrap_or_default()),
                            )
                        },
                    )
                    .when(
                        crate::server::enabled() && self.server_status.is_some(),
                        |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .bottom(px(102.))
                                    .left(px(24.))
                                    .right(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(12.))
                                    .text_size(px(12.))
                                    .text_color(t.muted)
                                    .child(self.server_status.clone().unwrap_or_default())
                                    .when(self.server_pending == 0, |d| {
                                        d.child(
                                            div()
                                                .id("retry-server-transcription")
                                                .px(px(10.))
                                                .py(px(6.))
                                                .rounded(px(8.))
                                                .bg(t.fill)
                                                .cursor_pointer()
                                                .child(
                                                    crate::motion::tactile(neutral_wash(0.14))
                                                        .radius(8.),
                                                )
                                                .child("Retry transcription")
                                                .reach().on_click(cx.listener(move |this, _, _, cx| {
                                                    this.server_action(
                                                        id,
                                                        crate::server::Action::Sync,
                                                        cx,
                                                    )
                                                })),
                                        )
                                    }),
                            )
                        },
                    )
                    .when(
                        owned
                            && std::env::args()
                                .any(|arg| arg.starts_with("--view=") || arg == "--smoke-test"),
                        |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .bottom(px(7.))
                                    .left_0()
                                    .right_0()
                                    .text_center()
                                    .text_size(px(10.))
                                    .line_height(px(12.))
                                    .text_color(t.muted)
                                    .child(format!(
                                        "Recording UI preview · microphone off · {}",
                                        crate::recording::elapsed_label(self.recording.elapsed())
                                    )),
                            )
                        },
                    ),
            );
        // Both notices leave the way they arrived instead of cutting out.
        let offer_generate = !live
            && !generating
            && (!crate::server::enabled()
                || (self.server_pending == 0 && !note.transcript.trim().is_empty()))
            && note.enhanced.trim().is_empty()
            && (owned || !note.body.trim().is_empty() || !note.transcript.trim().is_empty());
        let generate =
            self.stage
                .presence(("generate-notes", id), offer_generate, crate::motion::GOOEY);
        if generate.visible {
            let expanded_height = if self.transcript_open {
                Some(
                    (window.viewport_size().height - px(100.))
                        .min(px(465.))
                        .max(px(280.)),
                )
            } else if self.assistant_open {
                Some(
                    (px(if self.chat_recipes_open {
                        465.
                    } else if self
                        .chat_messages
                        .iter()
                        .any(|(_, answer)| !answer.is_empty())
                    {
                        // (The history list opens over a conversation
                        // without resizing the panel: see the panel itself.)
                        465.
                    } else if self.chat_history_open {
                        360.
                    } else {
                        220.
                    }) + px(if self.chat_editing.is_some() { 44. } else { 0. }))
                    .min(window.viewport_size().height - px(90.)),
                )
            } else {
                None
            };
            // While it leaves it keeps the position it had.
            let bottom = if offer_generate {
                expanded_height
                    .map(|height| height + px(36.))
                    .unwrap_or(px(98.))
            } else {
                px(self
                    .stage
                    .target(("generate-notes-bottom", 0))
                    .unwrap_or(98.))
            };
            result = result.child(self.render_generate_notes_cta(id, bottom, generate, cx));
        }
        let offer_regenerate = !live
            && !generating
            && (!crate::server::enabled() || self.server_pending == 0)
            && note.needs_regeneration()
            && self.regeneration_notice_dismissed != Some((id, note.transcript_revision));
        let regenerate = self.stage.presence(
            ("regeneration-notice", id),
            offer_regenerate,
            crate::motion::GOOEY,
        );
        if regenerate.visible {
            result = result.child(self.render_regeneration_notice(
                id,
                note.transcript_revision,
                regenerate,
                cx,
            ));
        }
        if assistant.visible {
            result = result.child(self.render_note_assistant(assistant, window, cx));
        }
        if let Some(popover) = self.render_summary_popover(window, cx) {
            result = result.child(popover);
        }
        result.into_any_element()
    }

    fn render_transcript_panel(
        &mut self,
        id: u64,
        width: Pixels,
        height: Pixels,
        pose: crate::motion::Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        let note = self.workspace.note(id);
        let transcript = note
            .map(|note| note.transcript.as_str())
            .unwrap_or_default();
        let owned = self.recording.note_id == Some(id);
        let failed = owned && self.recording.phase == crate::recording::Phase::Failed;
        let live = owned && self.recording.is_active();
        let query = if self.transcript_find_open {
            self.search.read(cx).content().to_lowercase()
        } else {
            String::new()
        };
        let utterances = note.map(|note| self.utterances(note)).unwrap_or_default();
        // Text still being spoken follows the stored transcript. It is not
        // part of the note: it cannot be found, cited or copied.
        let speaking = note.map(|note| self.live.tail(note)).unwrap_or_default();
        let rows = Rows {
            stored: &utterances,
            live: &speaking,
        };
        // An open source highlight stays on its passage while the transcript
        // grows around it. Only the user closes it, or opens another.
        let seen = (
            note.map_or(0, |note| note.transcript_revision),
            utterances.len(),
        );
        let source_highlight = self
            .summary
            .transcript_selection
            .as_mut()
            .filter(|selection| selection.note_id == id)
            .and_then(|selection| selection.locate(&utterances, seen));
        if source_highlight.is_none() {
            self.summary.transcript_selection = None;
        }
        let matches = transcript_find_matches(&utterances, &query);
        let live_revision = self.live.revision();
        let reopened = self.transcript_last_epoch != self.transcript_epoch;
        let grew = transcript != self.transcript_last_text
            || live_revision != self.transcript_last_live;
        if query != self.transcript_last_query {
            self.transcript_find_index = 0;
            if let Some(first) = matches.first() {
                crate::lazy::reveal("transcript", first.utterance_index);
            }
        } else if query.is_empty()
            // Opening the panel shows the newest text, unless it was opened
            // on a source. After that it follows the tail only for a reader
            // who is already there: one who scrolled up is left alone.
            && ((reopened && source_highlight.is_none())
                || (!reopened
                    && grew
                    && self.transcript_scroll.max_offset().height
                        + self.transcript_scroll.offset().y
                        <= px(48.)))
        {
            self.transcript_scroll.scroll_to_bottom();
            crate::lazy::to_end("transcript");
        }
        if !matches.is_empty() {
            self.transcript_find_index %= matches.len();
        }
        let active_match = matches.get(self.transcript_find_index);
        self.transcript_last_epoch = self.transcript_epoch;
        self.transcript_last_live = live_revision;
        if self.transcript_last_text != transcript {
            self.transcript_last_text = transcript.to_string();
        }
        self.transcript_last_query = query.clone();
        // Lines that just arrived fade in, and text that just became final
        // firms up, once; everything else in the transcript is still.
        let line_levels: Vec<(u64, f32)> = self
            .live
            .line_slots()
            .map(|slot| {
                let level = self.stage.aim(
                    (crate::live_view::LINE_TRACK, slot),
                    1.,
                    crate::motion::SNAPPY,
                );
                (slot, level)
            })
            .collect();
        let line_level = |slot: Option<u64>| {
            slot.and_then(|slot| line_levels.iter().find(|(known, _)| *known == slot))
                .map_or(1., |(_, level)| level.clamp(0., 1.))
        };
        // Stored rows that are firming up, by row index (they are among the
        // newest few).
        let firming: Vec<(usize, f32)> = note
            .map(|note| {
                note.server_transcript_segments
                    .iter()
                    .rev()
                    .filter(|segment| !segment.text.trim().is_empty())
                    .take(8)
                    .enumerate()
                    .filter_map(|(back, segment)| {
                        let slot = self.live.firmed_slot(&segment.id)?;
                        Some((utterances.len().checked_sub(back + 1)?, line_level(Some(slot))))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let fresh: Vec<f32> = (0..speaking.len())
            .map(|index| line_level(self.live.fresh_slot(index)))
            .collect();
        self.live.settle_lines(|slot| {
            line_levels
                .iter()
                .find(|(known, _)| *known == slot)
                .is_none_or(|(_, level)| *level >= 1.)
        });
        let live_status = self.live.status(id);
        let status_pose = self.stage.presence(
            ("live-status", id),
            live_status.is_some(),
            crate::motion::GOOEY,
        );
        let live_status = live_status.or_else(|| self.live.last_status(id));
        let mut toolbar = div()
            .h(px(44.))
            .flex_shrink_0()
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(2.));
        if self.transcript_find_open {
            let mut find = div()
                .id("transcript-find-capsule")
                .w(px(220.))
                .h(px(32.))
                .pl(px(8.))
                .pr(px(4.))
                .rounded_full()
                .border(px(0.5))
                .border_color(t.border)
                .flex()
                .items_center()
                .gap(px(4.))
                .child(icon("search", 13., t.muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .child(self.search.clone()),
                );
            if !query.is_empty() {
                find = find.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(10.))
                        .line_height(px(14.))
                        .text_color(t.muted)
                        .whitespace_nowrap()
                        .child(if matches.is_empty() {
                            "No results".into()
                        } else {
                            format!("{}/{}", self.transcript_find_index + 1, matches.len())
                        }),
                );
            }
            if !matches.is_empty() {
                for previous in [true, false] {
                    let positions: Vec<_> = matches.iter().map(|m| m.utterance_index).collect();
                    let tip = if previous {
                        "Previous match"
                    } else {
                        "Next match"
                    };
                    find = find.child(
                        div()
                            .id(if previous {
                                "transcript-find-previous"
                            } else {
                                "transcript-find-next"
                            })
                            .size(px(20.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(crate::motion::tactile(t.hover).radius(4.))
                            .child(icon("chevron-down", 12., t.muted).with_transformation(
                                Transformation::rotate(radians(if previous {
                                    std::f32::consts::PI
                                } else {
                                    0.
                                })),
                            ))
                            .tooltip(move |_, cx| cx.new(|_| ToolTip::new(tip, t)).into())
                            .reach().on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(index) = transcript_find_step(
                                    this.transcript_find_index,
                                    positions.len(),
                                    previous,
                                ) {
                                    this.transcript_find_index = index;
                                    crate::lazy::reveal("transcript", positions[index]);
                                }
                                cx.notify();
                            })),
                    );
                }
                find = find.child(div().w(px(0.5)).h(px(12.)).bg(t.border));
            }
            toolbar = toolbar.child(
                find.child(
                    tool_tip("transcript-find-close", "close", "Close Find", t)
                        .size(px(20.))
                        .flex_shrink_0()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.transcript_find_open = false;
                            this.transcript_find_index = 0;
                            this.search
                                .update(cx, |input, cx| input.set_content("", cx));
                            this.focus.focus(window);
                            cx.notify();
                        })),
                ),
            );
        } else {
            toolbar = toolbar.child(
                tool_tip("transcript-find", "search", "Find in transcript", t).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.transcript_find_open = true;
                        this.summary.transcript_selection = None;
                        this.transcript_find_index = 0;
                        this.search.update(cx, |input, cx| {
                            input.set_content("", cx);
                            input.set_placeholder("Find", cx);
                        });
                        this.search.read(cx).focus_handle.focus(window);
                        cx.notify();
                    }),
                ),
            );
        }
        toolbar = toolbar
            .child(div().flex_1())
            .child(
                tool_tip("transcript-help", "help", "Recording help", t).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.focus.focus(window);
                        this.menu = Some(Menu::RecordingHelp);
                        cx.notify();
                    },
                )),
            )
            .child(
                tool_tip("transcript-copy", "copy", "Copy transcript", t).on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(note) = this.workspace.note(id) {
                            if note.transcript.is_empty() {
                                this.show_toast("No transcript to copy yet", cx);
                                return;
                            }
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                note.transcript.clone(),
                            ));
                            // Confirmed on the button itself.
                            crate::ui::confirm("transcript-copy", cx);
                        }
                    },
                )),
            )
            .child(
                div()
                    .id("transcript-collapse")
                    .size(px(28.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(crate::motion::tactile(t.hover).radius(6.))
                    .active(|d| d.opacity(0.7))
                    .child(div().w(px(12.)).h(px(1.)).bg(t.muted))
                    .reach().on_click(cx.listener(|this, _, window, cx| {
                        this.transcript_open = false;
                        this.summary.transcript_selection = None;
                        this.transcript_find_open = false;
                        this.search
                            .update(cx, |input, cx| input.set_content("", cx));
                        this.focus.focus(window);
                        cx.notify();
                    })),
            );
        let mut body = div()
            .id("transcript-panel-scroll")
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.transcript_scroll);
        // The spacing around the notice is padding of a wrapper, not margin:
        // a scroll container measures its content from its first child's box
        // to its last child's, so a margin above the first child (or below
        // the last) would be cut from the scroll range and the newest line
        // of a transcript that is being followed would sit under the footer.
        body = body.child(div().w_full().flex_shrink_0().pt(px(14.)).pb(px(4.)).child(
            div()
                .id("transcript-consent")
                .h(px(20.))
                .mx(px(8.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(t.muted)
                .cursor_pointer()
                .map(|link| {
                    crate::motion::dims(link, crate::motion::reveal_key("consent-notice"), 0.25)
                })
                .child("Always get consent when transcribing others. Learn more ›")
                .reach().on_click(cx.listener(|this, _, window, cx| {
                    this.focus.focus(window);
                    this.menu = Some(Menu::RecordingHelp);
                    cx.notify();
                })),
        ));
        if rows.is_empty() {
            let label = if crate::server::enabled() && live {
                "Listening…"
            } else if crate::server::enabled() && self.server_pending > 0 {
                "Transcribing…"
            } else if crate::server::enabled() {
                "Transcript Paused"
            } else if !crate::audio::preview_mode() {
                "Audio is saved locally. Transcription is not connected."
            } else if live && !failed {
                "Waiting for speech…"
            } else {
                "Transcript Paused"
            };
            body = body
                .flex()
                .items_center()
                .justify_center()
                .pb(px(52.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.muted)
                        .child(label),
                );
        } else {
            // Only the bubbles near the viewport are built; the rest keep
            // their measured height as spacers, so an hour-long transcript
            // scrolls and animates like a short one.
            let voices = transcript_voices(rows);
            let keys: Vec<u64> = (0..rows.len())
                .map(|index| transcript_key(index, rows, voices, source_highlight))
                .collect();
            let stored = utterances.len();
            let ids: Vec<u64> = rows.iter().map(|row| row.id).collect();
            let mut counter = LineCounter::new(13., cx);
            body = body.child(crate::lazy::estimated(
                "transcript",
                &ids,
                &keys,
                f32::from(width),
                0.,
                &self.transcript_scroll,
                window,
                |index| {
                    transcript_row_estimate(
                        index,
                        rows,
                        voices,
                        width - px(8.),
                        source_highlight,
                        &mut counter,
                    )
                },
                |index| {
                    // Interim text is half strength; it and the text that
                    // replaces it ease to their level instead of popping.
                    let level = if index >= stored {
                        0.5 * fresh.get(index - stored).copied().unwrap_or(1.)
                    } else {
                        firming
                            .iter()
                            .find(|(row, _)| *row == index)
                            .map_or(1., |(_, level)| 0.5 + 0.5 * level)
                    };
                    transcript_bubble(
                        index,
                        rows,
                        voices,
                        t,
                        width - px(8.),
                        &query,
                        active_match,
                        source_highlight,
                        level,
                    )
                },
            ));
        }
        // One quiet line under the newest text when the live transcript is
        // behind, reconnecting or unavailable. Absent when all is well.
        if status_pose.visible
            && let Some(status) = live_status
        {
            let ink = if status.warning { t.warning_ink } else { t.muted };
            body = body.child(
                div().w_full().flex_shrink_0().px(px(12.)).pb(px(8.)).child(
                    div()
                        .id("transcript-live-status")
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(5.))
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(ink)
                        .opacity(status_pose.fade)
                        .when(status.warning, |line| line.child(icon("warning", 12., ink)))
                        .child(status.text),
                ),
            );
        }
        let (warning_ink, warning_fill) = (t.warning_ink, t.warning_fill);
        let inner = div()
            .absolute()
            .bottom_0()
            .left_0()
            .w(width)
            .h_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .when(failed, |d| {
                d.child(
                    div()
                        .mx(px(8.))
                        .mt(px(4.))
                        .mb(px(8.))
                        .h(px(36.))
                        .flex_shrink_0()
                        .px(px(12.))
                        .rounded_full()
                        .border_1()
                        .border_color(t.warning_edge)
                        .bg(warning_fill)
                        .text_color(warning_ink)
                        .text_size(px(13.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        // A drawn glyph: a text "⚠︎" sits wherever the
                        // fallback font's baseline puts it.
                        .child(icon("warning", 17., warning_ink))
                        .child(div().flex_1().min_w_0().truncate().child(
                            if crate::audio::preview_mode() {
                                "We’re having trouble transcribing your meeting."
                            } else {
                                "An audio input could not record. Check its permission or device."
                            },
                        ))
                        .child(
                            div()
                                .id("transcript-help-guide")
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .map(|link| {
                                    crate::motion::dims(
                                        link,
                                        crate::motion::reveal_key("transcript-help-guide"),
                                        0.25,
                                    )
                                })
                                .child("Help guide")
                                .child(icon("share", 12., warning_ink))
                                .reach().on_click(cx.listener(|this, _, window, cx| {
                                    this.focus.focus(window);
                                    this.menu = Some(Menu::RecordingHelp);
                                    cx.notify();
                                })),
                        )
                        .child(
                            pill("transcript-troubleshooting", "Troubleshooting", "", t)
                                .h(px(26.))
                                .text_size(px(11.))
                                .border_color(t.warning_edge)
                                .text_color(warning_ink)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.focus.focus(window);
                                    this.menu = Some(Menu::RecordingHelp);
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(body)
            .child(crate::ui::scroll_thumb("transcript", &self.transcript_scroll))
            .child(separator(t))
            .child(
                div()
                    .h(px(64.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.render_recording_control(id, cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                tool_tip(
                                    "transcript-audio-settings",
                                    "sliders-vertical",
                                    "Audio settings",
                                    t,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.focus.focus(window);
                                        this.menu = Some(Menu::RecordingSettings);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                div()
                                    .id("transcript-language")
                                    .h(px(28.))
                                    .px(px(6.))
                                    .rounded(px(6.))
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .text_size(px(13.))
                                    .text_color(t.muted)
                                    .cursor_pointer()
                                    .child(crate::motion::tactile(t.hover).radius(6.))
                                    .child(div().text_size(px(11.)).child("文A"))
                                    .child(if crate::server::enabled() {
                                        "English"
                                    } else if !crate::audio::preview_mode() {
                                        "Transcription unavailable"
                                    } else if self.transcription_language.starts_with("English") {
                                        "English"
                                    } else {
                                        "Multi-language"
                                    })
                                    .child(icon("chevron-down", 12., t.muted))
                                    .reach().on_click(cx.listener(|this, _, window, cx| {
                                        this.focus.focus(window);
                                        this.menu = Some(Menu::TranscriptionLanguage);
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        div()
            .absolute()
            .bottom(px(22.))
            .left(px(16.))
            .right(px(16.))
            .flex()
            .justify_center()
            // The shell grows at paint time; the transcript inside keeps its
            // final layout and is simply uncovered, so nothing reflows.
            .child(crate::motion::pop(
                div()
                    .relative()
                    .w(width)
                    .h(height)
                    .rounded(px(32.))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .border_1()
                    .border_color(t.border)
                    .bg(t.card)
                    .overflow_hidden()
                    .shadow_sm()
                    .child(inner),
                crate::motion::Pop::unfold(59., f32::from(height)),
                pose.solid(),
            ))
            .into_any_element()
    }

    pub fn render_note_details(&self, cx: &mut Context<Self>) -> Div {
        if crate::server::enabled() {
            return self.render_connected_note_details(cx);
        }
        let t = self.theme();
        let note = self.current_note.and_then(|id| self.workspace.note(id));
        let date = note
            .and_then(|note| NaiveDate::parse_from_str(&note.date, "%Y-%m-%d").ok())
            .unwrap_or_else(|| Local::now().date_naive());
        let time = note.map(|note| note.time.as_str()).unwrap_or("");
        let mut people = div().flex().flex_col().gap(px(3.));
        people = people.child(
            div()
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(20.))
                        .rounded_full()
                        .bg(t.fill)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .text_color(t.muted)
                        .child("M"),
                )
                .child(div().text_size(px(13.)).child("Me")),
        );
        if let Some(note) = note {
            for (index, name) in note.participants.iter().enumerate() {
                let remove = name.clone();
                let initial = name
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>())
                    .unwrap_or_else(|| "?".into());
                people = people.child(
                    div()
                        .id(("note-attendee", index))
                        .h(px(30.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(5.))
                        .child(crate::motion::tactile(t.hover).radius(5.))
                        .child(
                            div()
                                .size(px(20.))
                                .rounded_full()
                                .bg(t.fill)
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(10.))
                                .text_color(t.muted)
                                .child(initial),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(13.))
                                .child(name.clone()),
                        )
                        .child(
                            tool_tip(
                                ("remove-note-attendee", index),
                                "close",
                                "Remove attendee",
                                t,
                            )
                            .size(px(22.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(note) =
                                        this.current_note.and_then(|id| this.workspace.note_mut(id))
                                    {
                                        note.participants.retain(|name| name != &remove);
                                    }
                                    this.persist(cx);
                                },
                            )),
                        ),
                );
            }
        }
        div()
            .w(px(280.))
            .rounded(px(POPOVER_RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.card)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("note-select-calendar-event")
                    .h(px(58.))
                    .px(px(12.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(3.))
                    .cursor_pointer()
                    .child(crate::motion::tactile(t.bg).radius(0.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .child("Select calendar event")
                            .child(icon("chevron-down", 12., t.muted)),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .text_color(t.muted)
                            .child(format!("{} · {}", date.format("%a, %b %-d"), time)),
                    )
                    .reach().on_click(cx.listener(|this, _, window, cx| {
                        this.search.update(cx, |input, cx| {
                            input.set_content("", cx);
                            input.set_placeholder("Search event...", cx);
                        });
                        this.menu = Some(Menu::CalendarEvent);
                        this.search.read(cx).focus_handle.focus(window);
                        cx.notify();
                    })),
            )
            .child(separator(t))
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .child(self.attendee_input.clone()),
            )
            .child(separator(t))
            .child(
                div()
                    .pt(px(8.))
                    .pb(px(5.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .px(px(8.))
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(t.muted)
                            .child("My workspace"),
                    )
                    .child(people),
            )
    }

    pub fn render_event_picker(&self, cx: &mut Context<Self>) -> Div {
        if crate::server::enabled() {
            return self.render_connected_event_picker(cx);
        }
        let t = self.theme();
        let query = self.search.read(cx).content();
        let events = self.calendar_events(&query);
        let mut list = div()
            .id("note-event-results")
            .max_h(px(250.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if events.is_empty() {
            list = list.child(
                div()
                    .px(px(12.))
                    .py(px(18.))
                    .text_size(px(13.))
                    .text_color(t.muted)
                    .child("No events found"),
            );
        }
        for event in events {
            let date = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d")
                .map(|date| date.format("%a, %b %-d").to_string())
                .unwrap_or_else(|_| event.date.clone());
            let label = format!("{} · {}", date, event.time);
            let id = event.id;
            list = list.child(
                div()
                    .id(("note-event", id))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(6.))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(crate::motion::tactile(t.bg).radius(6.))
                    .child(
                        div()
                            .mt(px(5.))
                            .size(px(10.))
                            .rounded(px(3.))
                            .bg(rgb(0x87e5e8)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .truncate()
                                    .child(event.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(14.))
                                    .text_color(t.muted)
                                    .child(label),
                            ),
                    )
                    .reach().on_click(cx.listener(move |this, _, window, cx| {
                        this.select_calendar_event(id, cx);
                        this.focus.focus(window);
                    })),
            );
        }
        div()
            .w(px(266.))
            .rounded(px(POPOVER_RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.card)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .child(self.search.clone()),
            )
            .child(separator(t))
            .child(list)
    }

    pub(crate) fn render_note_assistant(
        &mut self,
        pose: crate::motion::Pose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        let available_width = window.viewport_size().width
            - px(if self.current_note.is_none() {
                192. * self.sidebar_level
            } else {
                0.
            });
        let width = px(682.).min(available_width - px(32.));
        let has_answer = self
            .chat_messages
            .iter()
            .any(|(_, answer)| !answer.is_empty());
        // The recent-chats list drops down inside the panel. Over a
        // conversation the panel is already tall enough and keeps its
        // height: it used to shrink to 360, which moved the header, and the
        // button that had just been clicked, 105 px down from under the
        // pointer, so the click meant to close the list landed on the note
        // and closed the chat instead. An empty chat still grows to fit it.
        let height = (px(if self.chat_recipes_open || has_answer {
            465.
        } else if self.chat_history_open {
            360.
        } else {
            220.
        }) + px(if self.chat_editing.is_some() { 44. } else { 0. }))
        .min(window.viewport_size().height - px(90.));
        let edit_banner = self.stage.presence(
            ("chat-edit-banner", 0),
            self.chat_editing.is_some(),
            crate::motion::SNAPPY,
        );
        // Growing for an answer, recipes or an edit banner rides a spring
        // instead of jumping to the new height.
        let height = px(self.stage.aim(
            ("assistant-height", 0),
            f32::from(height),
            crate::motion::PANEL,
        ));
        let mut messages = div()
            .p(px(16.))
            .pt(px(if has_answer { 60. } else { 16. }))
            .flex()
            .flex_col();
        if self.chat_messages.is_empty() {
            messages = messages.child(self.render_chat_recipe_suggestions(cx));
        }
        for (index, (question, answer)) in self.chat_messages.iter().enumerate() {
            messages = messages.child(self.render_chat_turn(index, question, answer, true, cx));
        }
        if let Some(queue) = self.render_chat_queue(true, cx) {
            messages = messages.child(queue);
        }
        if has_answer && self.chat_messages.len() == 1 && self.chat_stream.is_none() {
            messages = messages.child(self.render_chat_recipe_suggestions(cx));
        }
        let header = if self.chat_recipes_open {
            self.render_chat_recipes_header(cx)
        } else {
            div()
                .relative()
                .h(px(48.))
                .border_b_1()
                .border_color(t.border)
                .flex_shrink_0()
                .px(px(14.))
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    tool_tip("note-chat-history", "chat-history", "Recent chats", t)
                        .size(px(26.))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_chat_history(cx))),
                )
                .child(icon("chevron-down", 12., t.muted))
                .child(div().flex_1())
                .child(
                    tool_tip("new-note-chat", "edit", "New chat", t)
                        .size(px(28.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.clear_chat(cx);
                            this.ask.read(cx).focus_handle.focus(window);
                        })),
                )
                .child(
                    tool_tip("maximize-note-chat", "chat-maximize", "Maximize", t)
                        .size(px(28.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.current_note.is_some() {
                                this.chat_maximized = true;
                            } else {
                                this.screen = crate::app::Screen::Chat;
                                this.assistant_open = false;
                            }
                            this.sidebar_open = true;
                            this.chat_history_open = false;
                            this.focus.focus(window);
                            cx.notify();
                        })),
                )
        };
        let body = if self.chat_recipes_open {
            self.render_chat_recipes(cx).into_any_element()
        } else {
            div()
                .id("note-chat-scroll")
                .track_scroll(&self.chat_scroll)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                .child(messages)
                .into_any_element()
        };
        // The scroll-to-latest button and the history popover leave as they
        // arrive.
        let scroll_down = self.stage.presence(
            ("chat-scroll-down", 0),
            !self.chat_recipes_open && self.chat_can_scroll_down(),
            crate::motion::SNAPPY,
        );
        let history = self.stage.presence(
            ("chat-history", 0),
            self.chat_history_open,
            crate::motion::GOOEY,
        );
        let inner = div()
            .absolute()
            .bottom(px(0.))
            .w_full()
            .h_full()
            .rounded(px(32.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(body)
            // The edit warning takes 44 px (36 + 8). The panel's height
            // already springs to make room; the banner opens into that room
            // and closes with it instead of appearing in one frame.
            .when(edit_banner.visible, |d| {
                let open = edit_banner.value.clamp(0., 1.);
                d.child(
                    div()
                        .h(px(44. * open))
                        .flex_shrink_0()
                        .overflow_hidden()
                        .opacity(edit_banner.fade)
                        .px(px(4.))
                        .child(self.render_chat_edit_banner(cx)),
                )
            })
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(4.))
                    .child(self.render_chat_composer_focused(
                        self.ask.read(cx).focus_handle.is_focused(window),
                        cx,
                    )),
            )
            .when(scroll_down.visible, |d| {
                d.child(crate::motion::pop(
                    self.render_chat_scroll_down(cx),
                    crate::motion::Pop::chip((0.5, 1.)),
                    scroll_down,
                ))
            })
            .when(history.visible, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(36.))
                        .left(px(12.))
                        .child(crate::motion::pop(
                            self.render_chat_history(cx),
                            crate::motion::Pop::menu((0.08, 0.)),
                            history,
                        ))
                        // A press anywhere else closes the list, as it does
                        // on the full chat page. (In an empty chat the panel
                        // grows to fit the list and the button moves up, so
                        // the list could not be closed from where it was
                        // opened.)
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_chat_history(cx))),
                )
            })
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if event.button == MouseButton::Left && this.chat_recipe_editor.is_none() {
                    this.close_chat_surface(cx);
                }
            }));
        // The card owns the whole shell (fill, border, shadow) so it can morph
        // out of the composer bar as one shape; see `motion::Morph`.
        crate::motion::pop(
            div()
                .absolute()
                .bottom(px(22.))
                .left((available_width - width) / 2. + px(1.))
                .w(width)
                .h(height + px(2.))
                .rounded(px(32.))
                .border_1()
                .border_color(t.border)
                .bg(t.card)
                .shadow_xl()
                .overflow_hidden()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(inner),
            crate::motion::Pop::unfold(59., f32::from(height) + 2.),
            pose.solid(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TranscriptUtterance {
    /// Which row this is, however the rows around it change: from the stored
    /// segment's ID, or the interim utterance's. A windowed list keeps the
    /// reader's place by it.
    pub id: u64,
    pub key: String,
    pub speaker: Option<String>,
    /// Provider speaker IDs are scoped to an independently transcribed part.
    pub part: Option<usize>,
    pub own: bool,
    pub source: &'static str,
    pub timestamp: Option<String>,
    pub text: String,
    pub is_final: bool,
}

/// The passage a source citation opened. It is remembered by its text, not
/// by the transcript's revision, so it stays highlighted while a recording
/// keeps adding to the transcript (stored text never changes; it is only
/// added to). It ends when the user closes it or opens another source.
pub struct TranscriptSelection {
    note_id: u64,
    /// Where the passage was last seen, and the transcript state then.
    utterance_index: usize,
    seen: (u64, usize),
    text: String,
}

impl TranscriptSelection {
    /// The passage's text, for diagnostics.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The row the passage is on now, or `None` if it is no longer in the
    /// transcript. `seen` identifies the transcript state, so the search
    /// runs only when that has changed.
    fn locate(&mut self, utterances: &[TranscriptUtterance], seen: (u64, usize)) -> Option<usize> {
        let holds = |index: usize| {
            utterances
                .get(index)
                .is_some_and(|row| row.text == self.text)
        };
        if self.seen == seen && self.utterance_index < utterances.len() {
            return Some(self.utterance_index);
        }
        // Text is normally appended after the passage, which leaves it where
        // it was. Otherwise it is the nearest row with the same words.
        let index = if holds(self.utterance_index) {
            self.utterance_index
        } else {
            (0..utterances.len())
                .filter(|index| holds(*index))
                .min_by_key(|index| index.abs_diff(self.utterance_index))?
        };
        self.utterance_index = index;
        self.seen = seen;
        Some(index)
    }
}

/// The rows of a transcript panel: the note's stored utterances, then the
/// lines that are still being spoken. Two slices rather than one list, so an
/// interim update never copies an hour of transcript.
#[derive(Clone, Copy)]
struct Rows<'a> {
    stored: &'a [TranscriptUtterance],
    live: &'a [TranscriptUtterance],
}

impl<'a> Rows<'a> {
    fn len(self) -> usize {
        self.stored.len() + self.live.len()
    }
    fn is_empty(self) -> bool {
        self.len() == 0
    }
    fn get(self, index: usize) -> Option<&'a TranscriptUtterance> {
        match index.checked_sub(self.stored.len()) {
            None => self.stored.get(index),
            Some(live) => self.live.get(live),
        }
    }
    fn at(self, index: usize) -> &'a TranscriptUtterance {
        self.get(index).expect("a transcript row index is within its rows")
    }
    fn iter(self) -> impl Iterator<Item = &'a TranscriptUtterance> {
        self.stored.iter().chain(self.live)
    }
}

impl TranscriptUtterance {
    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.text.to_lowercase().contains(query)
            || self
                .speaker
                .as_ref()
                .is_some_and(|speaker| speaker.to_lowercase().contains(query))
            || self
                .timestamp
                .as_ref()
                .is_some_and(|timestamp| timestamp.contains(query))
    }
}

pub(crate) fn transcript_quote_target(
    note: &Note,
    segment_id: Option<&str>,
    quote: &str,
    revision: Option<u64>,
) -> Result<usize, &'static str> {
    if revision.is_some_and(|revision| revision != note.transcript_revision) {
        return Err("The transcript changed. Open the source again.");
    }
    if quote.is_empty() {
        return Err("This source quotation is unavailable.");
    }
    if let Some(segment_id) = segment_id {
        return note
            .server_transcript_segments
            .iter()
            .filter(|segment| !segment.text.trim().is_empty())
            .position(|segment| segment.id == segment_id && segment.text.contains(quote))
            .ok_or("This source quotation is no longer available.");
    }
    transcript_utterances(note)
        .iter()
        .position(|row| row.text.contains(quote))
        .ok_or("This source quotation is no longer available.")
}

impl Cereal {
    pub(crate) fn jump_to_transcript_quote(
        &mut self,
        note_id: u64,
        segment_id: Option<&str>,
        quote: &str,
        revision: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .workspace
            .note(note_id)
            .filter(|note| !note.trashed)
            .ok_or("This meeting is no longer available.")
            .and_then(|note| transcript_quote_target(note, segment_id, quote, revision));
        let target = match target {
            Ok(target) => target,
            Err(error) => {
                self.show_toast(error, cx);
                return;
            }
        };
        self.open_note(note_id, cx);
        self.chat_maximized = false;
        self.summary.dismiss();
        self.assistant_open = false;
        self.transcript_open = true;
        self.transcript_epoch = self.transcript_epoch.wrapping_add(1);
        self.transcript_find_open = false;
        self.search.update(cx, |input, cx| {
            input.set_placeholder("Find in transcript", cx);
            input.set_content("", cx);
        });
        self.summary.transcript_selection = self.workspace.note(note_id).and_then(|note| {
            let utterances = self.utterances(note);
            Some(TranscriptSelection {
                note_id,
                utterance_index: target,
                seen: (note.transcript_revision, utterances.len()),
                text: utterances.get(target)?.text.clone(),
            })
        });
        self.transcript_find_index = 0;
        self.transcript_last_query.clear();
        crate::lazy::reveal("transcript", target);
        self.focus.focus(window);
        cx.notify();
    }
}

pub(crate) fn transcript_utterances(note: &crate::model::Note) -> Vec<TranscriptUtterance> {
    if !note.server_transcript_segments.is_empty() {
        return server_transcript_utterances(&note.server_transcript_segments);
    }
    legacy_transcript_utterances(&note.transcript)
}

pub(crate) fn server_transcript_utterances(
    segments: &[meeting_notes_api_types::TranscriptSegmentDTO],
) -> Vec<TranscriptUtterance> {
    let mut parts = Vec::new();
    for segment in segments
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
    {
        if let Some(scope) = transcript_part_scope(segment)
            && !parts.contains(&scope)
        {
            parts.push(scope);
        }
    }
    segments
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .map(|segment| {
            let own = segment.channel == meeting_notes_api_types::AudioChannel::Mic;
            let part = transcript_part_scope(segment).and_then(|scope| {
                parts
                    .iter()
                    .filter(|(microphone, _)| *microphone == own)
                    .position(|candidate| *candidate == scope)
                    .map(|index| index + 1)
            });
            let label = segment
                .speaker_label
                .as_deref()
                .filter(|label| !label.trim().is_empty());
            let speaker = if let Some(part) = part {
                Some(format!(
                    "{} · {} · Part {part}",
                    label.unwrap_or("Unlabeled speaker"),
                    if own { "Mic" } else { "System" }
                ))
            } else {
                label.map(str::to_owned)
            };
            TranscriptUtterance {
                id: crate::lazy::key(("segment", &segment.id)),
                key: format!("{:?}:{}", segment.channel, segment.speaker_id),
                speaker,
                part,
                own,
                source: if own { "Microphone" } else { "System audio" },
                timestamp: Some(format!(
                    "{:02}:{:02}",
                    segment.start_ms.max(0) / 60000,
                    segment.start_ms.max(0) / 1000 % 60
                )),
                text: segment.text.clone(),
                is_final: segment.is_final,
            }
        })
        .collect()
}

fn transcript_part_scope(
    segment: &meeting_notes_api_types::TranscriptSegmentDTO,
) -> Option<(bool, uuid::Uuid)> {
    let mut components = segment.speaker_id.split(':');
    let source = components.next()?;
    let chunk = uuid::Uuid::parse_str(components.next()?).ok()?;
    let speaker = components.next()?;
    let own = segment.channel == meeting_notes_api_types::AudioChannel::Mic;
    if components.next().is_some()
        || source != if own { "mic" } else { "system" }
        || !(speaker == "unknown"
            || speaker.strip_prefix("speaker_").is_some_and(|number| {
                !number.is_empty() && number.chars().all(|c| c.is_ascii_digit())
            }))
    {
        return None;
    }
    Some((own, chunk))
}

pub(crate) fn legacy_transcript_utterances(transcript: &str) -> Vec<TranscriptUtterance> {
    // Compatibility for local/offline notes. An anonymous block does not assert
    // that the voice was ours; only explicit Me/You source headers align right.
    transcript
        .split("\n\n")
        .enumerate()
        .filter_map(|(position, block)| {
            let mut lines = block.lines();
            let first = lines.next()?.trim();
            let rest = lines.collect::<Vec<_>>().join("\n");
            let (speaker, timestamp, text) = match first.rsplit_once(char::is_whitespace) {
                Some((speaker, time))
                    if !rest.is_empty()
                        && time.split(':').count() >= 2
                        && time.chars().all(|c| c.is_ascii_digit() || c == ':') =>
                {
                    (
                        Some(speaker.trim().to_string()),
                        Some(time.to_string()),
                        rest,
                    )
                }
                _ => (None, None, block.trim().to_string()),
            };
            if text.trim().is_empty() {
                return None;
            }
            let own = speaker.as_ref().is_some_and(|speaker| {
                matches!(
                    speaker.to_lowercase().as_str(),
                    "me" | "you" | "self" | "microphone"
                )
            });
            let (text, is_final) = text
                .strip_suffix(" [interim]")
                .map(|text| (text.to_string(), false))
                .unwrap_or((text, true));
            Some(TranscriptUtterance {
                id: crate::lazy::key(("block", position)),
                key: speaker.clone().unwrap_or_default(),
                speaker,
                part: None,
                own,
                source: if own { "Microphone" } else { "Transcript" },
                timestamp,
                text,
                is_final,
            })
        })
        .collect()
}

/// Lowercased search positions are mapped back to the original UTF-8 bytes so
/// highlighted source text remains exact, including expanding Unicode folds.
pub(crate) fn transcript_match_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() {
        return vec![];
    }
    let mut lower = String::new();
    let mut original = Vec::new();
    for (start, character) in text.char_indices() {
        let end = start + character.len_utf8();
        for folded in character.to_lowercase() {
            lower.push(folded);
            original.extend(std::iter::repeat_n((start, end), folded.len_utf8()));
        }
    }
    let query = query.to_lowercase();
    lower
        .match_indices(&query)
        .filter_map(|(start, matched)| {
            let first = original.get(start)?;
            let last = original.get(start + matched.len() - 1)?;
            Some(first.0..last.1)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TranscriptFindMatch {
    utterance_index: usize,
    text_range: Option<std::ops::Range<usize>>,
}

fn transcript_find_matches(
    utterances: &[TranscriptUtterance],
    query: &str,
) -> Vec<TranscriptFindMatch> {
    if query.is_empty() {
        return vec![];
    }
    let mut matches = Vec::new();
    for (utterance_index, utterance) in utterances.iter().enumerate() {
        let ranges = transcript_match_ranges(&utterance.text, query);
        if ranges.is_empty() && utterance.matches(&query.to_lowercase()) {
            // Preserve the existing ability to find explicit speaker/time metadata.
            matches.push(TranscriptFindMatch {
                utterance_index,
                text_range: None,
            });
        } else {
            matches.extend(ranges.into_iter().map(|range| TranscriptFindMatch {
                utterance_index,
                text_range: Some(range),
            }));
        }
    }
    matches
}

fn transcript_find_step(current: usize, count: usize, previous: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let current = current % count;
    Some(if previous {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    })
}

/// Every utterance of a transcript as an element. The panel itself builds
/// only the ones near its viewport (see [`transcript_bubble`] and `lazy`).
#[cfg(test)]
fn transcript_content(
    utterances: &[TranscriptUtterance],
    t: Theme,
    width: Pixels,
    query: &str,
    active_match: Option<&TranscriptFindMatch>,
    source_highlight: Option<usize>,
) -> Vec<AnyElement> {
    let rows = Rows {
        stored: utterances,
        live: &[],
    };
    let voices = transcript_voices(rows);
    (0..rows.len())
        .map(|index| {
            transcript_bubble(
                index,
                rows,
                voices,
                t,
                width,
                query,
                active_match,
                source_highlight,
                1.,
            )
        })
        .collect()
}

/// How many distinct voices the microphone and the system audio carry: a
/// speaker's name is only shown when a channel has more than one. Words
/// still being spoken that nobody has been named for yet are not a voice:
/// they would put names above the stored text for the moment it takes
/// their final to arrive, and take them away again if it turns out to be
/// the same speaker.
fn transcript_voices(rows: Rows) -> (usize, usize) {
    let voices = |own: bool| {
        rows.iter()
            .filter(|row| row.own == own && (row.is_final || row.speaker.is_some()))
            .map(|row| &row.key)
            .collect::<std::collections::HashSet<_>>()
            .len()
    };
    (voices(true), voices(false))
}

/// Whether utterance `index` continues the voice before it, and is continued
/// by the one after: consecutive bubbles of one voice sit closer together.
fn transcript_neighbours(index: usize, rows: Rows) -> (bool, bool) {
    let key = &rows.at(index).key;
    (
        index > 0 && rows.at(index - 1).key == *key,
        rows.get(index + 1).is_some_and(|next| next.key == *key),
    )
}

/// The name drawn above each row of a transcript (stored rows, then the
/// rows still being spoken), or `None` where a row has no heading. For
/// diagnostics and tests: it is exactly what the panel decides.
pub(crate) fn transcript_headings(
    stored: &[TranscriptUtterance],
    live: &[TranscriptUtterance],
) -> Vec<Option<String>> {
    let rows = Rows { stored, live };
    let voices = transcript_voices(rows);
    (0..rows.len())
        .map(|index| {
            transcript_names_speaker(index, rows, voices)
                .then(|| rows.at(index).speaker.clone())
                .flatten()
        })
        .collect()
}

/// Whether utterance `index` is headed by its speaker's name.
fn transcript_names_speaker(index: usize, rows: Rows, voices: (usize, usize)) -> bool {
    let utterance = rows.at(index);
    let multiple_voices = if utterance.own {
        voices.0 > 1
    } else {
        voices.1 > 1
    };
    !transcript_neighbours(index, rows).0
        && (multiple_voices || utterance.part.is_some())
        && utterance.speaker.is_some()
}

/// Everything that decides how tall utterance `index` is drawn (at a given
/// width): its text, its spacing from its neighbours, whether it is named and
/// whether it carries the source highlight. A bubble is measured once per key.
fn transcript_key(
    index: usize,
    rows: Rows,
    voices: (usize, usize),
    source_highlight: Option<usize>,
) -> u64 {
    // Not the index itself: a row keeps its key, and so its measured height,
    // when rows are inserted above it.
    crate::lazy::key((
        index == 0,
        &rows.at(index).text,
        transcript_neighbours(index, rows),
        index + 1 == rows.len(),
        transcript_names_speaker(index, rows, voices),
        source_highlight == Some(index),
    ))
}

/// A guess of how tall [`transcript_bubble`] will draw row `index`, from the
/// same numbers it lays out with and a quick count of the text's lines. Rows
/// that have not been on screen stand in the scroll range at this height.
fn transcript_row_estimate(
    index: usize,
    rows: Rows,
    voices: (usize, usize),
    width: Pixels,
    source_highlight: Option<usize>,
    counter: &mut LineCounter,
) -> f32 {
    let (same_before, same_after) = transcript_neighbours(index, rows);
    let highlighted = source_highlight == Some(index);
    // The bubble's column, less the highlight's and the bubble's padding.
    let column = (f32::from(width) - 80.).max(1.);
    let text_width = column - if highlighted { 8. } else { 0. } - 16.;
    let lines = counter.lines(&rows.at(index).text, text_width) as f32;
    let label = if transcript_names_speaker(index, rows, voices) {
        16. + 2.
    } else {
        0.
    };
    let highlight = if highlighted { 8. } else { 0. };
    let margins = if same_before { 0. } else { 4. }
        + if index == 0 { 6. } else { 0. }
        + if same_after { 0. } else { 4. }
        + if index + 1 == rows.len() { 6. } else { 0. };
    lines * 17.875 + 12. + label + highlight + 2. + margins
}

/// One utterance of the transcript.
#[allow(clippy::too_many_arguments)]
fn transcript_bubble(
    index: usize,
    rows: Rows,
    voices: (usize, usize),
    t: Theme,
    width: Pixels,
    query: &str,
    active_match: Option<&TranscriptFindMatch>,
    source_highlight: Option<usize>,
    // Strength of the bubble: 1 for stored text, 0.5 for text still being
    // spoken, in between while one is becoming the other.
    level: f32,
) -> AnyElement {
    let dark = t.bg.l < 0.5;
    {
        let utterance = rows.at(index);
        let (same_before, same_after) = transcript_neighbours(index, rows);
        let own = utterance.own;
        let group: SharedString = format!("transcript-utterance-{index}").into();
        let highlights = transcript_match_ranges(&utterance.text, query)
            .into_iter()
            .map(|range| {
                (
                    range.clone(),
                    HighlightStyle {
                        background_color: Some(
                            if active_match.is_some_and(|active| {
                                active.utterance_index == index
                                    && active.text_range.as_ref() == Some(&range)
                            }) {
                                rgb(0xf4a340).into()
                            } else {
                                rgb(0xfde047).into()
                            },
                        ),
                        color: Some(rgb(0x292929).into()),
                        ..Default::default()
                    },
                )
            });
        let text = StyledText::new(utterance.text.clone()).with_highlights(highlights);
        let mut bubble = div()
            .relative()
            .max_w_full()
            .min_w_0()
            .px(px(8.))
            .py(px(6.))
            .rounded(px(8.))
            .when(same_before && own, |d| d.rounded_tr(px(6.)))
            .when(same_before && !own, |d| d.rounded_tl(px(6.)))
            .when(same_after && own, |d| d.rounded_br(px(6.)))
            .when(same_after && !own, |d| d.rounded_bl(px(6.)))
            .text_size(px(13.))
            .line_height(px(17.875))
            .bg(if dark {
                if own { rgb(0x292929) } else { rgb(0x4e4d4b) }
            } else if own {
                rgba(0xe0af2f66)
            } else {
                rgb(0xf2f2ec)
            })
            .text_color(if dark {
                if own { rgb(0xfcf2df) } else { rgb(0xfcfcf8) }
            } else if own {
                rgb(0x363024)
            } else {
                rgb(0x292929)
            })
            .when(!utterance.is_final || level < 1., |d| {
                d.opacity(if utterance.is_final { level } else { level.min(0.5) })
            })
            .child(text);
        if let Some(timestamp) = &utterance.timestamp {
            bubble = bubble.child(
                div()
                    .absolute()
                    .top(px(6.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .whitespace_nowrap()
                    .text_color(t.muted)
                    .opacity(0.)
                    .group_hover(group.clone(), |d| d.opacity(1.))
                    .when(own, |d| d.right(relative(1.)).mr(px(8.)))
                    .when(!own, |d| d.left(relative(1.)).ml(px(8.)))
                    .child(timestamp.clone()),
            );
        }
        let label = [
            Some(utterance.source.to_string()),
            utterance.speaker.clone(),
            // The time is already shown beside the bubble on hover.
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let mut stack = div()
            // Give the column a definite wrapping width. Percentage-only
            // max-width on nested flex columns can measure the text at zero
            // width, leaving the row's tooltip hitbox but no painted bubble.
            .w((width - px(80.)).max(px(1.)))
            .min_w_0()
            .max_w_full()
            .flex()
            .flex_col()
            .gap(px(2.))
            .when(source_highlight == Some(index), |stack| {
                stack.p(px(4.)).rounded(px(10.)).bg(if dark {
                    rgba(0xa99afc38)
                } else {
                    rgba(0x9276df30)
                })
            })
            .when(own, |d| d.items_end())
            .when(!own, |d| d.items_start());
        if transcript_names_speaker(index, rows, voices)
            && let Some(speaker) = &utterance.speaker
        {
            stack = stack.child(
                div()
                    .px(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child(speaker.clone()),
            );
        }
        stack = stack.child(bubble);
        {
            div()
                .id(("transcript-bubble", index))
                .group(group)
                .w(width - px(16.))
                .min_w_0()
                .flex_shrink_0()
                .whitespace_normal()
                .flex()
                .mx(px(12.))
                .py(px(1.))
                .mt(px(
                    if !same_before { 4. } else { 0. } + if index == 0 { 6. } else { 0. }
                ))
                .mb(px(if !same_after { 4. } else { 0. }
                    + if index + 1 == rows.len() {
                        6.
                    } else {
                        0.
                    }))
                .when(own, |d| d.pl(px(64.)).justify_end())
                .when(!own, |d| d.pr(px(64.)).justify_start())
                .child(stack)
                .tooltip(move |_, cx| cx.new(|_| ToolTip::new(label.clone(), t)).into())
                .into_any_element()
        }
    }
}

/// Deterministic formatting only: keeps the user's words and groups existing
/// lines that explicitly describe an action. No generated facts or AI service.
pub fn local_outline(body: &str) -> String {
    let mut notes = Vec::new();
    let mut actions = Vec::new();
    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let text = line
            .trim_start_matches('#')
            .trim()
            .trim_start_matches("- ")
            .trim_start_matches("• ")
            .trim_start_matches("* ");
        if text.is_empty()
            || text
                .chars()
                .all(|c| c.is_whitespace() || matches!(c, '-' | '*' | '#' | '_' | '•'))
        {
            continue;
        }
        let lower = text.to_lowercase();
        let is_action = lower.starts_with("todo:")
            || lower.starts_with("action:")
            || lower.starts_with("next step:")
            || lower.contains(" to share ")
            || lower.contains(" to review ")
            || lower.contains(" to send ");
        if is_action {
            actions.push(format!("- {text}"));
        } else {
            notes.push(format!("- {text}"));
        }
    }
    let mut sections = Vec::new();
    if !notes.is_empty() {
        sections.push(format!("## Notes\n\n{}", notes.join("\n")));
    }
    if !actions.is_empty() {
        sections.push(format!("## Action items\n\n{}", actions.join("\n")));
    }
    sections.join("\n\n")
}

#[cfg(test)]
mod metadata_tests {
    use super::{metadata_anchors, note_date_label};
    use crate::app::NoteTab;
    use chrono::NaiveDate;

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn metadata_day_labels_handle_year_and_leap_day_boundaries() {
        assert_eq!(
            note_date_label(date("2026-01-01"), date("2026-01-01")),
            "Today"
        );
        assert_eq!(
            note_date_label(date("2025-12-31"), date("2026-01-01")),
            "Yesterday"
        );
        assert_eq!(
            note_date_label(date("2024-02-29"), date("2024-03-01")),
            "Yesterday"
        );
        assert_eq!(
            note_date_label(date("2024-02-28"), date("2024-03-01")),
            "Feb 28"
        );
        assert_eq!(
            note_date_label(date("2024-03-02"), date("2024-03-01")),
            "Mar 2"
        );
    }

    #[test]
    fn popovers_follow_the_joined_views_and_natural_metadata_width() {
        let enhanced = metadata_anchors(1200., true, NoteTab::Enhanced, 151.);
        assert_eq!(enhanced.details_left, 444.);
        assert_eq!(enhanced.folder_left, 603.);
        let raw_view = metadata_anchors(1200., true, NoteTab::Notes, 151.);
        assert_eq!(raw_view.details_left, 444.);
        assert_eq!(raw_view.folder_left, 603.);
        let ungenerated = metadata_anchors(1200., false, NoteTab::Notes, 128.);
        assert_eq!(ungenerated.details_left, 293.);
        assert_eq!(ungenerated.folder_left, 429.);
        let longer_label = metadata_anchors(1200., true, NoteTab::Enhanced, 200.);
        assert_eq!(longer_label.details_left, enhanced.details_left);
        assert_eq!(longer_label.folder_left - enhanced.folder_left, 49.);
    }

    #[test]
    fn popovers_track_resizing_and_stay_within_compact_window_edges() {
        let wide = metadata_anchors(1200., true, NoteTab::Enhanced, 151.);
        let compact = metadata_anchors(760., true, NoteTab::Enhanced, 151.);
        assert_eq!(wide.details_left - compact.details_left, 220.);
        assert_eq!(wide.folder_left - compact.folder_left, 220.);
        let narrow = metadata_anchors(500., true, NoteTab::Enhanced, 151.);
        assert!(narrow.details_left >= 12. && narrow.details_left + 280. <= 488.);
        assert!(narrow.folder_left >= 12. && narrow.folder_left + 288. <= 488.);
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::{
        transcript_content, transcript_find_matches, transcript_find_step, transcript_match_ranges,
        transcript_utterances,
    };
    use crate::model::Note;
    use meeting_notes_api_types::{AudioChannel, TranscriptSegmentDTO};

    #[test]
    fn evidence_targets_the_issued_segment_not_the_first_duplicate_quote_and_rejects_stale_sources()
    {
        let mut first = segment(
            AudioChannel::Mic,
            "speaker-first",
            "Speaker 1",
            "Same café quotation 🦊.\nNext line.",
        );
        first.id = "segment-first".into();
        let mut second = first.clone();
        second.id = "segment-second".into();
        second.channel = AudioChannel::System;
        let note = Note {
            server_transcript_segments: vec![first, second],
            transcript_revision: 7,
            ..Default::default()
        };
        assert_eq!(
            super::transcript_quote_target(
                &note,
                Some("segment-second"),
                "Same café quotation 🦊.\nNext line.",
                Some(7)
            ),
            Ok(1)
        );
        assert!(
            super::transcript_quote_target(
                &note,
                Some("segment-second"),
                "Same café quotation 🦊.",
                Some(6)
            )
            .is_err()
        );
        assert!(
            super::transcript_quote_target(
                &note,
                Some("deleted-segment"),
                "Same café quotation 🦊.",
                Some(7)
            )
            .is_err()
        );
        assert!(
            super::transcript_quote_target(
                &note,
                Some("segment-second"),
                "Invented quotation",
                Some(7)
            )
            .is_err()
        );
        assert!(super::transcript_quote_target(&note, None, "", Some(7)).is_err());
    }

    fn segment(
        channel: AudioChannel,
        speaker: &str,
        label: &str,
        text: &str,
    ) -> TranscriptSegmentDTO {
        TranscriptSegmentDTO {
            id: format!("segment-{speaker}"),
            channel,
            speaker_id: speaker.into(),
            speaker_label: Some(label.into()),
            text: text.into(),
            is_final: true,
            start_ms: 18_000,
            end_ms: Some(19_000),
        }
    }

    #[test]
    fn actual_channels_control_alignment_without_guessing_speaker_identity() {
        let note = Note {
            transcript: "Flattened text is not the rendering source".into(),
            server_transcript_segments: vec![
                segment(
                    AudioChannel::Mic,
                    "first",
                    "Speaker 0",
                    "An in-room speaker.",
                ),
                segment(
                    AudioChannel::Mic,
                    "second",
                    "Speaker 1",
                    "Another in-room speaker.",
                ),
                segment(
                    AudioChannel::System,
                    "remote",
                    "Me",
                    "A system source with a misleading display name.",
                ),
            ],
            ..Note::default()
        };
        let rows = transcript_utterances(&note);
        assert_eq!(rows.len(), 3);
        assert!(rows[0].own && rows[1].own);
        assert_ne!(rows[0].key, rows[1].key);
        assert_eq!(rows[0].speaker.as_deref(), Some("Speaker 0"));
        assert_eq!(rows[1].speaker.as_deref(), Some("Speaker 1"));
        assert!(!rows[2].own);
        assert_eq!(rows[2].source, "System audio");
        assert_eq!(rows[0].timestamp.as_deref(), Some("00:18"));
    }

    #[test]
    fn canonical_speakers_expose_channel_and_part_without_cross_chunk_identity_claims() {
        let first = "419d65d1-7a2c-44ca-84cc-ad489522d561";
        let second = "419d65d1-7a2c-44ca-84cc-ad489522d562";
        let mut unknown = segment(
            AudioChannel::System,
            &format!("system:{second}:unknown"),
            "",
            "Unknown system voice",
        );
        unknown.speaker_label = None;
        let rows = super::server_transcript_utterances(&[
            segment(
                AudioChannel::Mic,
                &format!("mic:{first}:speaker_0"),
                "Speaker 1",
                "First part",
            ),
            segment(
                AudioChannel::Mic,
                &format!("mic:{first}:speaker_1"),
                "Speaker 2",
                "Second voice",
            ),
            segment(
                AudioChannel::Mic,
                &format!("mic:{second}:speaker_0"),
                "Speaker 1",
                "Next independent part",
            ),
            segment(
                AudioChannel::System,
                &format!("system:{first}:speaker_0"),
                "Speaker 1",
                "System part",
            ),
            unknown,
        ]);
        assert_eq!(rows[0].speaker.as_deref(), Some("Speaker 1 · Mic · Part 1"));
        assert_eq!(rows[1].speaker.as_deref(), Some("Speaker 2 · Mic · Part 1"));
        assert_eq!(rows[2].speaker.as_deref(), Some("Speaker 1 · Mic · Part 2"));
        assert_eq!(
            rows[3].speaker.as_deref(),
            Some("Speaker 1 · System · Part 1")
        );
        assert_eq!(
            rows[4].speaker.as_deref(),
            Some("Unlabeled speaker · System · Part 2")
        );
        assert_ne!(rows[0].key, rows[2].key);
        assert_eq!(
            rows.iter().map(|row| row.part).collect::<Vec<_>>(),
            vec![Some(1), Some(1), Some(2), Some(1), Some(2)]
        );
        assert!(rows[0].own && !rows[3].own);
    }

    #[test]
    fn real_transcript_text_and_finality_remain_exact() {
        let mut actual = segment(
            AudioChannel::System,
            "remote",
            "Zoë 李",
            "  Keep café, 😀 and [interim] literally.\nNext line.  ",
        );
        actual.is_final = false;
        let note = Note {
            server_transcript_segments: vec![actual.clone()],
            ..Note::default()
        };
        let rows = transcript_utterances(&note);
        assert_eq!(rows[0].text, actual.text);
        assert!(!rows[0].is_final);
        assert!(rows[0].matches("café"));
        assert!(rows[0].matches("zoë"));
        assert!(rows[0].matches("00:18"));
    }

    #[test]
    fn legacy_preview_headers_are_hidden_without_claiming_anonymous_blocks_are_me() {
        let note = Note { transcript: "You  00:18\nHello there.\n\nSomeone else  00:20\nTheir answer.\n\nAn unattributed block.".into(), ..Note::default() };
        let rows = transcript_utterances(&note);
        assert!(rows[0].own);
        assert_eq!(rows[0].text, "Hello there.");
        assert!(!rows[1].own);
        assert_eq!(rows[1].text, "Their answer.");
        assert!(!rows[2].own);
        assert_eq!(rows[2].speaker, None);
        assert_eq!(rows[2].text, "An unattributed block.");
    }

    #[test]
    fn transcript_search_keeps_original_unicode_byte_boundaries() {
        let text = "Café İSTANBUL 😀";
        let ranges = transcript_match_ranges(text, "CAFÉ");
        assert_eq!(&text[ranges[0].clone()], "Café");
        let ranges = transcript_match_ranges(text, "i\u{307}stanbul");
        assert_eq!(&text[ranges[0].clone()], "İSTANBUL");
        let ranges = transcript_match_ranges(text, "😀");
        assert_eq!(&text[ranges[0].clone()], "😀");
        assert!(transcript_match_ranges(text, "missing").is_empty());
    }

    #[test]
    fn find_counts_individual_occurrences_without_filtering_source_rows() {
        let note = Note {
            server_transcript_segments: vec![
                segment(
                    AudioChannel::Mic,
                    "me",
                    "Speaker 0",
                    "Budget first; budget again.",
                ),
                segment(
                    AudioChannel::System,
                    "remote",
                    "Speaker 1",
                    "No matching word here.",
                ),
                segment(
                    AudioChannel::System,
                    "remote",
                    "Speaker 1",
                    "The BUDGET is agreed.",
                ),
            ],
            ..Note::default()
        };
        let rows = transcript_utterances(&note);
        let original = rows.clone();
        let matches = transcript_find_matches(&rows, "budget");
        assert_eq!(matches.len(), 3);
        assert_eq!(
            matches
                .iter()
                .map(|m| m.utterance_index)
                .collect::<Vec<_>>(),
            vec![0, 0, 2]
        );
        assert_eq!(
            matches
                .iter()
                .map(|m| &rows[m.utterance_index].text[m.text_range.clone().unwrap()])
                .collect::<Vec<_>>(),
            vec!["Budget", "budget", "BUDGET"],
        );
        let rendered = transcript_content(
            &rows,
            crate::ui::Theme::new(true),
            gpui::px(674.),
            "budget",
            matches.first(),
            None,
        );
        assert_eq!(
            rendered.len(),
            3,
            "The unmatched middle utterance must remain rendered"
        );
        assert_eq!(
            rows, original,
            "Find must not alter chronology, channels, names or source text"
        );
    }

    fn row(text: &str) -> super::TranscriptUtterance {
        super::TranscriptUtterance {
            id: crate::lazy::key(text),
            key: "System:system:n:1".into(),
            speaker: Some("Speaker 1".into()),
            part: None,
            own: false,
            source: "System audio",
            timestamp: Some("00:10".into()),
            text: text.into(),
            is_final: true,
        }
    }

    #[test]
    fn an_open_source_highlight_survives_text_arriving_after_and_before_it() {
        let mut rows: Vec<_> = ["First point.", "The cited passage.", "A later point."]
            .map(row)
            .into();
        let mut selection = super::TranscriptSelection {
            note_id: 1,
            utterance_index: 1,
            seen: (7, rows.len()),
            text: "The cited passage.".into(),
        };
        assert_eq!(selection.locate(&rows, (7, 3)), Some(1));
        // A recording keeps appending: every few seconds a new revision.
        for revision in 8..40 {
            rows.push(row(&format!("Live segment {revision}.")));
            assert_eq!(selection.locate(&rows, (revision, rows.len())), Some(1));
        }
        // The other channel finishes an earlier sentence late: the passage
        // moves down a row and the highlight moves with it.
        rows.insert(0, row("An earlier sentence."));
        assert_eq!(selection.locate(&rows, (40, rows.len())), Some(2));
        // The same words elsewhere do not steal it.
        rows.push(row("The cited passage."));
        assert_eq!(selection.locate(&rows, (41, rows.len())), Some(2));
        // Only when the passage itself is gone does the highlight end.
        rows.retain(|row| row.text != "The cited passage.");
        assert_eq!(selection.locate(&rows, (42, rows.len())), None);
    }

    #[test]
    fn interim_rows_follow_the_stored_ones_without_joining_them() {
        let stored: Vec<_> = ["One.", "Two."].map(row).into();
        let mut speaking = row("and thr");
        speaking.is_final = false;
        let speaking = [speaking];
        let rows = super::Rows {
            stored: &stored,
            live: &speaking,
        };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.at(1).text, "Two.");
        assert_eq!((rows.at(2).text.as_str(), rows.at(2).is_final), ("and thr", false));
        assert!(rows.get(3).is_none());
        // Same voice throughout: the interim bubble continues the group, so
        // its final text will sit exactly where it is.
        assert_eq!(super::transcript_neighbours(1, rows), (true, true));
        assert_eq!(super::transcript_neighbours(2, rows), (true, false));
        // A row's key changes with its text (its height may), not otherwise.
        let voices = super::transcript_voices(rows);
        let key = super::transcript_key(2, rows, voices, None);
        assert_eq!(key, super::transcript_key(2, rows, voices, None));
        let mut longer = speaking.clone();
        longer[0].text.push_str("ee");
        let grown = super::Rows {
            stored: &stored,
            live: &longer,
        };
        assert_ne!(key, super::transcript_key(2, grown, voices, None));
        // Stored rows are not re-keyed by interim text changing below them,
        // except the last one, whose spacing depends on what follows.
        assert_eq!(
            super::transcript_key(0, rows, voices, None),
            super::transcript_key(0, grown, voices, None)
        );
        // Finding looks at stored text only.
        assert!(transcript_find_matches(&stored, "thr").is_empty());
    }

    #[test]
    fn no_find_results_still_render_the_entire_transcript() {
        let note = Note {
            transcript: "You  00:18\nFirst café.\n\nSomeone else  00:20\nSecond 😀.".into(),
            ..Note::default()
        };
        let rows = transcript_utterances(&note);
        assert!(transcript_find_matches(&rows, "absent search").is_empty());
        assert_eq!(
            transcript_content(
                &rows,
                crate::ui::Theme::new(true),
                gpui::px(674.),
                "absent search",
                None,
                None,
            )
            .len(),
            2
        );
        assert_eq!(rows[0].text, "First café.");
        assert_eq!(rows[1].text, "Second 😀.");
    }

    #[test]
    fn find_navigation_wraps_and_unicode_matches_keep_exact_source_ranges() {
        assert_eq!(transcript_find_step(0, 0, false), None);
        assert_eq!(transcript_find_step(0, 3, true), Some(2));
        assert_eq!(transcript_find_step(2, 3, false), Some(0));
        assert_eq!(transcript_find_step(15, 4, false), Some(0));
        assert_eq!(transcript_find_step(0, 1, true), Some(0));
        let note = Note {
            transcript: "İSTANBUL 😀 café CAFÉ".into(),
            ..Note::default()
        };
        let rows = transcript_utterances(&note);
        for (query, expected) in [
            ("i\u{307}stanbul", vec!["İSTANBUL"]),
            ("😀", vec!["😀"]),
            ("CAFÉ", vec!["café", "CAFÉ"]),
        ] {
            let matches = transcript_find_matches(&rows, query);
            assert_eq!(
                matches
                    .iter()
                    .map(|m| &rows[m.utterance_index].text[m.text_range.clone().unwrap()])
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
