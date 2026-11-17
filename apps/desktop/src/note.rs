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
}

/// The document is centered independently of sidebar state on a note page.
/// Use the same content gutter and joined-tab width as the actual metadata
/// row; `joined_views` is `None` while the note has no enhanced notes.
fn metadata_anchors(viewport_width: f32, joined_views: Option<f32>) -> NoteMetadataAnchors {
    let document_width = viewport_width.min(if joined_views.is_some() { 682. } else { 672. });
    let gutter_left = (viewport_width - document_width) / 2. + 29.;
    let details_left = gutter_left + joined_views.map_or(0., |width| width + 8.);
    let clamp = |left: f32, width: f32| left.clamp(12., (viewport_width - width - 12.).max(12.));
    NoteMetadataAnchors {
        details_left: clamp(details_left, 280.),
    }
}

/// What the joined view tabs of a note page show.
#[derive(Clone, Copy)]
pub(crate) struct NoteTabs {
    /// The first tab's label: "My notes" on a note of one's own.
    pub notes_label: &'static str,
    /// Whether the Enhanced tab carries its template-menu chevron.
    pub menu: bool,
}

// Geometry of the joined view tabs, in points, taken from the reference
// capture of this control (`.local/gooey/desktop/audit/15-…` and `24-…`, one
// pixel per point) by where the ink falls. The selected tab is a capsule
// exactly the track's size on its end of the track: no inset, so its radius
// is the track's. Each glyph starts about 13.5 pt from its segment's outer
// edge and stays there in both states; a widening tab uncovers its label.
// The `*_LEAD` values are the glyph boxes' insets that put the ink there.
const TAB_HEIGHT: f32 = 28.;
const NOTES_GLYPH: f32 = 18.;
const NOTES_LEAD: f32 = 11.1;
const NOTES_LABEL_GAP: f32 = 2.35;
const SPARKLE_GLYPH: f32 = 16.;
const SPARKLE_LEAD: f32 = 12.5;
/// Collapsed, the sparkle sits nearer the capsule beside it, as in the reference.
const SPARKLE_LEAD_COLLAPSED: f32 = 11.6;
const SPARKLE_LABEL_GAP: f32 = 5.9;
/// The template-menu chevron, and its gap after the label or the glyph.
const CHEVRON: f32 = 10.5;
const CHEVRON_GAP: f32 = 0.3;
const CHEVRON_GAP_COLLAPSED: f32 = 1.3;
/// After the last thing in a tab: a label, a chevron, or a lone glyph.
const LABEL_TRAIL: f32 = 13.2;
const CHEVRON_TRAIL: f32 = 7.8;
const CHEVRON_TRAIL_COLLAPSED: f32 = 10.9;
/// A collapsed tab without a chevron: the lines at the track's start, as in
/// the reference, and the sparkle at its end, as far from the end as the
/// lines are from the start.
const GLYPH_TAB: f32 = 35.;
const SPARKLE_TRAIL_COLLAPSED: f32 = 11.4;

/// The widths of the two view tabs, collapsed and selected. A collapsed tab
/// is as wide as its glyph (and chevron) need, never wider, so the track is
/// the selected tab plus the other's glyph. On a note's own page the two
/// states come out the same width, as the reference's do, so the metadata
/// chips beside it stay put.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NoteTabWidths {
    pub notes: f32,
    pub notes_collapsed: f32,
    pub enhanced: f32,
    pub enhanced_collapsed: f32,
}

impl NoteTabWidths {
    /// The track's width while `selected` is the selected tab.
    pub fn track(self, selected: NoteTab) -> f32 {
        match selected {
            NoteTab::Notes => self.notes + self.enhanced_collapsed,
            NoteTab::Enhanced => self.notes_collapsed + self.enhanced,
        }
    }
}

/// Lay out the view tabs for the two labels' drawn widths.
pub(crate) fn note_tab_widths(notes_label: f32, enhanced_label: f32, menu: bool) -> NoteTabWidths {
    let notes = NOTES_LEAD + NOTES_GLYPH + NOTES_LABEL_GAP + notes_label + LABEL_TRAIL;
    let (enhanced, enhanced_collapsed) = if menu {
        (
            SPARKLE_LEAD
                + SPARKLE_GLYPH
                + SPARKLE_LABEL_GAP
                + enhanced_label
                + CHEVRON_GAP
                + CHEVRON
                + CHEVRON_TRAIL,
            SPARKLE_LEAD_COLLAPSED
                + SPARKLE_GLYPH
                + CHEVRON_GAP_COLLAPSED
                + CHEVRON
                + CHEVRON_TRAIL_COLLAPSED,
        )
    } else {
        (
            SPARKLE_LEAD + SPARKLE_GLYPH + SPARKLE_LABEL_GAP + enhanced_label + LABEL_TRAIL,
            SPARKLE_LEAD_COLLAPSED + SPARKLE_GLYPH + SPARKLE_TRAIL_COLLAPSED,
        )
    };
    // Whole points, so the capsule's edges land on pixels at 1x too.
    NoteTabWidths {
        notes: notes.round(),
        notes_collapsed: GLYPH_TAB,
        enhanced: enhanced.round(),
        enhanced_collapsed: enhanced_collapsed.round(),
    }
}

/// The width of a view tab's label as it is drawn: 13 px in the body face.
pub(crate) fn note_tab_label_width(label: &'static str, window: &Window) -> f32 {
    let runs = [TextRun {
        len: label.len(),
        font: font(FONT),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }];
    f32::from(
        window
            .text_system()
            .shape_line(label.into(), px(13.), &runs, None)
            .width,
    )
}

/// The outlined capsule of a note's facts (its day, who was there): 28 px
/// tall, 13 px secondary text, 14 px icons 6 px apart.
pub(crate) fn note_meta_chip(id: &'static str, t: Theme) -> Stateful<Div> {
    div()
        .id(id)
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
}

impl Cereal {
    pub(crate) fn note_metadata_anchors(&self, window: &Window) -> NoteMetadataAnchors {
        let Some(note) = self.current_note.and_then(|id| self.workspace.note(id)) else {
            return metadata_anchors(f32::from(window.viewport_size().width), None);
        };
        let enhanced = !note.enhanced.trim().is_empty() || self.generation.is_running_for(note.id);
        let joined = enhanced.then(|| {
            note_tab_widths(
                note_tab_label_width("My notes", window),
                note_tab_label_width("Enhanced", window),
                true,
            )
            .track(self.active_tab)
        });
        metadata_anchors(f32::from(window.viewport_size().width), joined)
    }

    /// The joined capsule that switches a note between its two documents: a
    /// sunken track, and on it the selected tab as a raised capsule of the
    /// track's own height and radius, flush with the track's end. The
    /// capsule is one shape that flows across to the other tab (stretching
    /// like a droplet, see [`crate::motion::Glide`]) while both tabs trade
    /// widths on the same spring. `select` is told the tab that was pressed
    /// and whether it was already the active one.
    pub(crate) fn note_view_tabs(
        &self,
        tabs_shown: NoteTabs,
        display_tab: NoteTab,
        window: &Window,
        cx: &mut Context<Self>,
        select: fn(&mut Self, NoteTab, bool, &mut Window, &mut Context<Self>),
    ) -> Div {
        let t = self.theme();
        let tabs = self.stage.glide("note-tabs");
        let notes_label = note_tab_label_width(tabs_shown.notes_label, window);
        let widths = note_tab_widths(
            notes_label,
            note_tab_label_width("Enhanced", window),
            tabs_shown.menu,
        );
        // The track's edge is lighter than the selected capsule's, which
        // paints over it: one edge wherever they meet, never two.
        let track_edge = t.hairline.opacity(0.55);
        let pill_edge = Hsla {
            a: (t.hairline.a * 1.25).min(1.),
            ..t.hairline
        };
        // The selected tab's ink. Light mode uses the secondary ink, as the
        // reference does; dark mode goes halfway to the primary text, so the
        // selected label stands out from the chips beside it there too
        // (8.1:1 on the capsule, against 5.4:1 for the chips).
        let dark = t.bg.l < 0.5;
        let selected_ink = if dark {
            crate::motion::mix(t.secondary, t.text, 0.5)
        } else {
            t.secondary
        };
        let mut views = div()
            .relative()
            .h(px(TAB_HEIGHT))
            .flex()
            .items_center()
            .rounded_full()
            .bg(t.sidebar)
            .flex_shrink_0()
            // The capsule's spring may reach a fraction of a point past the
            // ends of the track; the track keeps it in.
            .overflow_hidden()
            // The edge is its own layer so the tabs fill the whole track
            // and the selected capsule can sit on its outer edge.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .border_1()
                    .border_color(track_edge),
            )
            .child(tabs.outlined_indicator(t.card, Some(pill_edge), TAB_HEIGHT / 2.));
        for (index, tab) in [NoteTab::Notes, NoteTab::Enhanced].into_iter().enumerate() {
            let active = display_tab == tab;
            let enhanced = tab == NoteTab::Enhanced;
            let chevron = enhanced && tabs_shown.menu;
            let (label, glyph, glyph_size, (lead_collapsed, lead), label_gap, collapsed, selected) =
                if enhanced {
                    (
                        "Enhanced",
                        "note-tab-sparkle",
                        SPARKLE_GLYPH,
                        (SPARKLE_LEAD_COLLAPSED, SPARKLE_LEAD),
                        SPARKLE_LABEL_GAP,
                        widths.enhanced_collapsed,
                        widths.enhanced,
                    )
                } else {
                    (
                        tabs_shown.notes_label,
                        "note-tab-lines",
                        NOTES_GLYPH,
                        (NOTES_LEAD, NOTES_LEAD),
                        NOTES_LABEL_GAP,
                        widths.notes_collapsed,
                        widths.notes,
                    )
                };
            let width = self.stage.aim(
                ("note-tab", index as u64),
                if active { selected } else { collapsed },
                crate::motion::GOOEY,
            );
            // How far this tab is open, 0 collapsed to 1 selected. The label,
            // the gaps and the ink all follow it, so a tab uncovers or covers
            // its label in step with its width and nothing jumps.
            let open = if selected > collapsed {
                ((width - collapsed) / (selected - collapsed)).clamp(0., 1.)
            } else if active {
                1.
            } else {
                0.
            };
            let lerp = |from: f32, to: f32| from + (to - from) * open;
            let ink = crate::motion::mix(t.muted, selected_ink, open);
            // The label is uncovered and covered by its tab's edge, but never
            // shown cut through a letter: it is only visible once the tab has
            // room for all of it (the last few tenths of `open`, inside the
            // last letter's side bearing), fades in from there, and is gone
            // within a frame or two of its tab starting to close.
            let room = ((open - 0.93) / 0.06).clamp(0., 1.);
            let arrived = self
                .stage
                .presence(
                    ("note-tab-label", index as u64),
                    open >= 0.97,
                    crate::motion::SNAPPY,
                )
                .fade;
            let label_fade = room * room * (3. - 2. * room) * arrived;
            let collapsed_trail = if chevron {
                CHEVRON_TRAIL_COLLAPSED
            } else {
                collapsed - lead_collapsed - glyph_size
            };
            let selected_trail = if chevron { CHEVRON_TRAIL } else { LABEL_TRAIL };
            let id = SharedString::from(format!("note-tab-{label}"));
            let element = div()
                .id(id)
                .h(px(TAB_HEIGHT))
                .w(px(width.max(0.)))
                .flex_shrink_0()
                .flex()
                .items_center()
                .rounded_full()
                .overflow_hidden()
                .whitespace_nowrap()
                .cursor_default()
                .pl(px(lerp(lead_collapsed, lead)))
                .pr(px(lerp(collapsed_trail, selected_trail).max(0.)))
                .text_size(px(13.))
                // A collapsed tab washes under the pointer like any tool; the
                // wash fades out as the tab opens into the raised capsule, so
                // the capsule stays white under the pointer and only darkens
                // while it is held.
                .child(
                    crate::motion::tactile(t.hover.opacity(1. - open))
                        .press(t.border.opacity(0.6))
                        .round(),
                )
                .when(active, |d| d.child(tabs.probe(index)))
                .child(icon(glyph, glyph_size, ink))
                // The label shrinks away rather than the chevron being pushed
                // out of a narrowing tab.
                .when(open > 0., |d| {
                    d.child(
                        div()
                            .ml(px(label_gap * open))
                            .min_w_0()
                            .flex_shrink()
                            .overflow_hidden()
                            .text_color(selected_ink)
                            .opacity(label_fade)
                            .child(label),
                    )
                })
                .when(chevron, |d| {
                    d.child(
                        icon("note-tab-chevron", CHEVRON, ink)
                            .ml(px(lerp(CHEVRON_GAP_COLLAPSED, CHEVRON_GAP))),
                    )
                })
                .reach()
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    // Pressed from the keyboard, the tab keeps the focus
                    // (`select` hands it to the page, which suits a click).
                    let focused = event.is_keyboard().then(|| window.focused(cx)).flatten();
                    select(this, tab, active, window, cx);
                    if let Some(handle) = focused {
                        window.focus(&handle);
                    }
                }));
            // The selected tab shows its name already; only a collapsed one,
            // a lone glyph, is named on hover.
            views = views.child(if active {
                element
            } else {
                with_tooltip(element, label.into(), t)
            });
        }
        views
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
        // Stop was pressed and the stream is still delivering what the
        // server does not have: the notes are asked for after that. Shown
        // in the generation chip's place, as the step before it.
        let finishing = !self.generation.is_running_for(id)
            && note.pending_stop == Some(crate::model::PendingStop::Generate)
            && (self.live.delivering(id) || !note.live_pending.is_empty());
        let generating = self.generation.is_running_for(id) || finishing;
        // The notes of a generation as far as they have been written: they
        // take the place of the chip (and of the notes being replaced) from
        // their first word, and the note's own text takes over from them,
        // word for word, once they are saved.
        let draft = (self.generation_draft.of(self.generation.epoch)
            && self.generation_draft.showing(id))
        .then(|| self.generation_draft.visible().to_owned());
        let has_enhanced = !note.enhanced.trim().is_empty() || generating || draft.is_some();
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
        let views = self.note_view_tabs(
            NoteTabs {
                notes_label: "My notes",
                menu: true,
            },
            display_tab,
            window,
            cx,
            |this, tab, active, window, cx| {
                this.focus.focus(window);
                if active && tab == NoteTab::Enhanced {
                    this.open_menu(Menu::Templates);
                } else {
                    this.summary.dismiss();
                    this.active_tab = tab;
                }
                cx.notify();
            },
        );
        // The generation chip has a presence of its own. If the user watched
        // the generation finish, the chip leaves and the notes arrive block
        // by block, once. If it finished while they were elsewhere, the
        // notes are simply there when they come back.
        let watched = self.stage.continuous(("note-view", id));
        let was_generating = self.stage.target(("generation", id)) == Some(1.);
        if !watched && !generating {
            self.stage.forget(("generation", id));
        }
        let chip = self.stage.presence(
            ("generation", id),
            generating && draft.is_none(),
            crate::motion::GOOEY,
        );
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
            // The chip and the placeholder over it take 88 px (the two and
            // their gaps). That block opens as generation starts and closes
            // up when it ends, with the chip: notes that are already there
            // (a regeneration) slide down to make room and the new ones
            // slide into place, instead of jumping by the block's height in
            // a single frame.
            NoteTab::Enhanced if generating || chip.visible || draft.is_some()=>div().w(body_width).flex().flex_col()
                // Clipped to the block (which is widened to the chip's
                // 24 px overhang), so the chip is covered as the notes rise
                // rather than drawn across their first heading.
                .child(div().h(px(88. * chip.value.clamp(0., 1.))).mx(px(-24.)).px(px(24.)).overflow_hidden().flex().flex_col().gap(px(12.))
                    .child(div().text_size(px(16.)).line_height(px(24.)).text_color(t.muted).opacity(chip.fade).child("Add your thoughts…"))
                    .child(self.render_generation_status(body_width + px(48.), chip, finishing)))
                .map(|d| match &draft {
                    // Read-only while it is being written: the same blocks,
                    // in the same places, as the saved notes will have.
                    Some(text) => {
                        let written = crate::model::Note {
                            enhanced: text.clone(),
                            enhanced_dirty: false,
                            enhanced_authorship: Default::default(),
                            ..note.clone()
                        };
                        d.child(self.render_note_markdown(&written, true, t, body_width, window, cx))
                    }
                    None if !note.enhanced.trim().is_empty() => d.child(self.render_enhanced_content(&note,t,body_width,window,cx)),
                    None => d,
                })
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
        let (date_label, people_label) = metadata_labels(&note, crate::menubar::now().date_naive());
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
                        note_meta_chip("note-details", t)
                            .cursor_default()
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
                                this.open_menu(Menu::NoteDetails);
                                cx.notify();
                            })),
                    )
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
            crate::transcript_panel::OPEN,
        );
        let assistant =
            self.stage
                .presence(("assistant", 0), self.assistant_open, crate::motion::PANEL);
        let owned = self.recording.note_id == Some(id);
        let live = owned && self.recording.is_active();
        // The bar's centre sits 56 pt above the window's bottom edge, as in
        // the reference, now that its controls are 56 pt tall.
        let ask_focused = self.ask.read(cx).focus_handle.is_focused(window);
        let ask_typed = !self.ask.read(cx).content().trim().is_empty();
        let footer = div()
            .absolute()
            .bottom(px(28.))
            .left(px(24.))
            .right(px(24.))
            .flex()
            .justify_center()
            .child(bar_scrim(t, 24.))
            // While the chat panel is up its bottom row is this row: the
            // record capsule and the Ask pill in the same places, merged
            // into the panel's body (`render_note_assistant`).
            .when(!assistant.visible, |footer| {
                footer.child(
                    div()
                        .w_full()
                        .max_w(px(672.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(self.render_recording_control(id, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                // A press anywhere outside the pill gives
                                // the focus back to the page (the press goes
                                // on to whatever it was aimed at).
                                .on_mouse_down_out(cx.listener(
                                    |this, event: &MouseDownEvent, window, cx| {
                                        if event.button == MouseButton::Left
                                            && !this.chat_popover_open()
                                            && this.ask.read(cx).focus_handle.is_focused(window)
                                        {
                                            this.focus.focus(window);
                                            cx.notify();
                                        }
                                    },
                                ))
                                .child(
                                    self.render_chat_composer_in_tray(
                                        ask_focused,
                                        !ask_focused && !ask_typed && self.chat_stream.is_none(),
                                        cx,
                                    )
                                    .min_h(px(NOTE_PILL)),
                                ),
                        ),
                )
            });
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
                        // The panel stands where the bar did, so the page
                        // under it ends the same way: nothing of the note
                        // shows in the strip between the panel and the
                        // bottom of the window.
                        d.child(
                            div()
                                .absolute()
                                .bottom(px(26.))
                                .left(px(24.))
                                .right(px(24.))
                                .h(px(58.))
                                .child(bar_scrim(t, 24.)),
                        )
                        .child({
                            // Where the bar stood, for the shape the panel
                            // grows out of and folds back into.
                            let bar = crate::transcript_panel::BarGeometry {
                                width: f32::from(window.viewport_size().width - px(48.)).min(672.),
                                lift: 6.,
                                capsule: self.closed_capsule_width(id, cx),
                                ask: !assistant.visible,
                            };
                            self.render_transcript_panel(
                                id,
                                (document_width + px(8.)).min(px(682.)),
                                (window.viewport_size().height - px(100.))
                                    .min(px(465.))
                                    .max(px(280.)),
                                transcript,
                                bar,
                                window,
                                cx,
                            )
                        })
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
                            // A chip of its own, like the generation
                            // status: the note scrolls behind it, and a
                            // bare line of text there was set on top of the
                            // note's own lines.
                            d.child(
                                div()
                                    .absolute()
                                    .bottom(px(96.))
                                    .left(px(24.))
                                    .right(px(24.))
                                    .flex()
                                    .justify_center()
                                    .child(
                                div()
                                    .h(px(32.))
                                    .pl(px(12.))
                                    .pr(px(12.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(t.border)
                                    .bg(t.card)
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(t.muted)
                                    .child(self.server_status.clone().unwrap_or_default()),
                                    ),
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
        // Nothing to offer for a recording in which next to nothing was
        // said: its notes would be empty.
        let spoken = !crate::server::enabled()
            || self
                .workspace
                .note(id)
                .is_some_and(|note| crate::recording_view::enough_speech(&note.transcript));
        let offer_generate = !live
            && !generating
            && draft.is_none()
            && spoken
            && (!crate::server::enabled()
                || (self.server_pending == 0 && !note.transcript.trim().is_empty()))
            && note.enhanced.trim().is_empty()
            && (owned || !note.body.trim().is_empty() || !note.transcript.trim().is_empty());
        if offer_generate {
            crate::latency::mark("generate_available");
        }
        let generate =
            self.stage
                .presence(("generate-notes", id), offer_generate, crate::motion::GOOEY);
        // What floats above the footer bar (the call to generate notes, the
        // notice that the transcript has changed) clears whichever panel is
        // open.
        let expanded_height = if self.transcript_open {
            Some(
                (window.viewport_size().height - px(100.))
                    .min(px(465.))
                    .max(px(280.)),
            )
        } else if self.assistant_open {
            Some(
                (px(if self
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
        if generate.visible {
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
        // The notice needs room above an open panel. In a short window a
        // panel reaches to just under the note's title, and the notice waits
        // until the panel is closed rather than lie across the title.
        let room_for_notice = expanded_height.is_none_or(|height| {
            window.viewport_size().height - (height + px(36.)) - px(54.) >= px(140.)
        });
        let offer_regenerate = !live
            && !generating
            && room_for_notice
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
                expanded_height
                    .map(|height| height + px(36.))
                    .unwrap_or(px(98.)),
                regenerate,
                window,
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
        bar: crate::transcript_panel::BarGeometry,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        // The footer names the microphone, and the sound popover's meter
        // listens while it is up.
        if self.transcript_last_epoch != self.transcript_epoch {
            self.refresh_input_devices();
        }
        self.tend_meter(cx);
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
        let ring = if source_highlight.is_some() {
            self.stage
                .presence(SOURCE_RING, true, crate::motion::GOOEY)
        } else {
            self.stage.forget(SOURCE_RING);
            crate::motion::Pose::REST
        };
        // The panel's content box: its outline is drawn inside `width`.
        let inside = width - px(2.);
        // The bubbles' column, with room for the scroll thumb.
        let column = inside - px(8.);
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
        let firming: Vec<(usize, f32, Option<(u64, SharedString)>)> = note
            .map(|note| {
                note.server_transcript_segments
                    .iter()
                    .rev()
                    .filter(|segment| !segment.text.trim().is_empty())
                    .take(8)
                    .filter_map(|segment| {
                        let slot = self.live.firmed_slot(&segment.id)?;
                        let id = crate::lazy::key(("segment", &segment.id));
                        // Among the newest rows (gap rows may stand between).
                        let row = utterances
                            .iter()
                            .enumerate()
                            .rev()
                            .take(16)
                            .find(|(_, row)| row.id == id)?
                            .0;
                        let replaced = self
                            .live
                            .replaced(&segment.id)
                            .map(|(key, text)| (key, SharedString::from(text.to_owned())));
                        Some((row, line_level(Some(slot)), replaced))
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
            toolbar = toolbar.child(self.transcript_find_field(
                &query,
                self.transcript_find_index,
                matches.iter().map(|found| found.utterance_index).collect(),
                cx,
            ));
        } else {
            toolbar = toolbar.child(
                tool_tip("transcript-find", "search", "Find in transcript", t).on_click(
                    cx.listener(|this, _, window, cx| this.transcript_find_begin(window, cx)),
                ),
            );
        }
        toolbar = toolbar.child(div().flex_1()).child(
            tool_tip("transcript-copy", "copy", "Copy transcript", t).on_click(cx.listener(
                move |this, _, _, cx| {
                    if let Some(note) = this.workspace.note(id) {
                        if note.transcript.is_empty() {
                            this.show_toast("No transcript to copy yet", cx);
                            return;
                        }
                        cx.write_to_clipboard(ClipboardItem::new_string(note.transcript.clone()));
                        // Confirmed on the button itself.
                        crate::ui::confirm("transcript-copy", cx);
                    }
                },
            )),
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
                .gap(px(4.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(t.muted)
                .cursor_pointer()
                .map(|link| {
                    crate::motion::dims(link, crate::motion::reveal_key("consent-notice"), 0.25)
                })
                .child("Always get consent when transcribing others.")
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.secondary)
                        .child("Learn more ›"),
                )
                .reach().on_click(cx.listener(|this, _, window, cx| {
                    this.focus.focus(window);
                    this.open_menu(Menu::RecordingHelp);
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
            let marks = transcript_marks(rows);
            let keys: Vec<u64> = (0..rows.len())
                .map(|index| transcript_key(index, rows, voices, &marks))
                .collect();
            let stored = utterances.len();
            let ids: Vec<u64> = rows.iter().map(|row| row.id).collect();
            let mut counter = LineCounter::new(13., cx);
            crate::lazy::hold_glide(
                "transcript",
                self.transcript_open
                    .then(|| transcript_glide_owner(id, self.transcript_epoch)),
            );
            body = body.child(crate::lazy::estimated(
                "transcript",
                &ids,
                &keys,
                f32::from(column),
                0.,
                &self.transcript_scroll,
                window,
                |index| {
                    transcript_row_estimate(
                        index,
                        rows,
                        voices,
                        column,
                        &marks,
                        &mut counter,
                    )
                },
                |index| {
                    // A line being spoken fades in as it appears; a final
                    // that replaced one settles in that same bubble.
                    let (level, live) = if index >= stored {
                        (
                            fresh.get(index - stored).copied().unwrap_or(1.),
                            Some(LiveLine {
                                settle: None,
                                inherits: None,
                                ghost: None,
                            }),
                        )
                    } else {
                        let line = firming.iter().find(|(row, _, _)| *row == index).map(
                            |(_, level, replaced)| LiveLine {
                                settle: Some(*level),
                                inherits: replaced.as_ref().map(|(key, _)| *key),
                                ghost: replaced.as_ref().map(|(_, text)| text.clone()),
                            },
                        );
                        (1., line)
                    };
                    transcript_bubble(
                        index,
                        rows,
                        voices,
                        &marks,
                        t,
                        column,
                        &query,
                        active_match,
                        source_highlight,
                        ring,
                        level,
                        live.as_ref(),
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
            .w(inside)
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
                                    this.open_menu(Menu::RecordingHelp);
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
                                    this.open_menu(Menu::RecordingHelp);
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(body)
            .child(crate::ui::scroll_thumb("transcript", &self.transcript_scroll))
            .child(separator(t))
            .child(
                // The record controls are drawn by the shape itself (they
                // ride from the bar into this footer), so the footer keeps
                // their room and holds the microphone, the sound settings
                // and the language at its end.
                div()
                    .h(px(64.))
                    .flex_shrink_0()
                    .pl(px(12. + bar.capsule + 12.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .child(self.transcript_footer_tools(cx)),
            );
        let inner = inner
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        // The record control, at its place in the footer: 12 pt in and
        // centred in the 64 pt strip. In the bar it stood `from` away.
        let controls = div()
            .absolute()
            .left(px(12.))
            .bottom(px(4.))
            .child(self.render_panel_recording_control(id, cx));
        let from = point(px((f32::from(width) - bar.width) / 2. - 12.), px(-2.));
        // What the bar's Ask capsule showed, fading as the capsules meet.
        let bar_left = (f32::from(width) - bar.width) / 2.;
        let leaving = div()
            .absolute()
            .left(px(bar_left + bar.capsule + 8. + 20.))
            .w(px((bar.width - bar.capsule - 8. - 31.).max(0.)))
            .bottom(px(bar.lift))
            .h(px(56.))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(22.))
            .child(self.ask.clone());
        let shape = crate::transcript_panel::fuse(
            (f32::from(width), f32::from(height)),
            bar,
            pose,
            t,
            div().absolute().inset_0().child(inner),
        )
        .pinned(controls, from)
        .when(bar.ask, |shape| shape.leaving(leaving));
        div()
            .absolute()
            .bottom(px(22.))
            .left(px(16.))
            .right(px(16.))
            .flex()
            .justify_center()
            // One shape with the note's bar: its two capsules fuse and grow
            // into the panel, and split back out of it. The transcript
            // inside keeps its final layout throughout, so nothing reflows.
            .child(shape)
            .into_any_element()
    }

    pub fn render_note_details(&self, cx: &mut Context<Self>) -> Div {
        if crate::server::enabled() {
            return self.render_connected_note_details(cx);
        }
        use crate::meeting::{DetailsCard, DetailsPerson};
        let note = self.current_note.and_then(|id| self.workspace.note(id));
        let when = note
            .map(|note| crate::meeting::note_when(None, &note.date, &note.time))
            .unwrap_or_default();
        // A note kept here has no account: its writer is "Me", or whoever a
        // layout review signed in.
        let session = crate::portrait::tour_session(cx);
        let picture = session
            .as_ref()
            .and_then(|session| crate::portrait::of(session, cx));
        let mut people = vec![match session {
            Some(session) => DetailsPerson {
                name: session.user.name,
                aside: "(me)",
                picture,
                removable: false,
            },
            None => DetailsPerson {
                name: "Me".into(),
                aside: "",
                picture: None,
                removable: false,
            },
        }];
        people.extend(note.into_iter().flat_map(|note| &note.participants).map(|name| {
            DetailsPerson {
                name: name.clone(),
                aside: "",
                picture: None,
                removable: true,
            }
        }));
        self.render_details_card(
            DetailsCard {
                title: None,
                when,
                links: None,
                status: Vec::new(),
                people,
                empty: None,
            },
            cx,
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
                    .cursor_default()
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
                self.sidebar_inset()
            } else {
                0.
            });
        // Over Home the panel has the footprint of the tray the focused
        // composer sits in (see `ask_bar`), 4 px around the bar it grows out
        // of: 683 px wide and 467 px tall as soon as a question is sent,
        // even before its answer arrives, as the reference measures. Over a
        // note it keeps the note's own bar.
        let home = self.current_note.is_none();
        // Either way the pill stands `ASK_INSET` inside the panel's edge on
        // its left, right and bottom (3 px of card inside the 1 px hairline),
        // and the panel's corners are concentric with the pill's.
        let width = if home {
            px(683.).min(available_width - px(40.))
        } else {
            // The note's bar (672 wide, 24 in from each side) with the
            // inset round it.
            px(672. + 2. * crate::motion::ASK_INSET)
                .min(available_width - px(48. - 2. * crate::motion::ASK_INSET))
        };
        // Tall as soon as a question is asked, on Home and over a note
        // alike: the panel does not grow again in a second jump when the
        // first words of the answer arrive.
        let has_answer = !self.chat_messages.is_empty();
        // The recent-chats list drops down inside the panel. Over a
        // conversation the panel is already tall enough and keeps its
        // height: it used to shrink to 360, which moved the header, and the
        // button that had just been clicked, 105 px down from under the
        // pointer, so the click meant to close the list landed on the note
        // and closed the chat instead. An empty chat still grows to fit it.
        let height = (px(if has_answer {
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
        // Growing for an answer or an edit banner rides a spring
        // instead of jumping to the new height.
        let height = px(self.stage.aim(
            ("assistant-height", 0),
            f32::from(height),
            crate::motion::PANEL,
        ));
        let mut messages = div()
            .p(px(16.))
            // The last row stops clear of the fade above the composer.
            .pb(px(30.))
            .pt(px(match (has_answer, home) {
                (true, true) => 33.,
                (true, false) => 60.,
                _ => 16.,
            }))
            .flex()
            .flex_col();
        if !self.chat_messages.is_empty() {
            messages = messages.child(self.render_chat_turns(1, true, window, cx));
        }
        if let Some(queue) = self.render_chat_queue(true, window, cx) {
            messages = messages.child(queue);
        }
        // What the panel grows out of: the pill's own outline, or the tray
        // the focused bar stood in on Home (whose footprint the panel's
        // lower 101 px are). Folding back, it always returns to the pill.
        let foot = if home {
            self.stage.aim(
                ("assistant-foot", 0),
                if self.assistant_open && self.stage.target(("ask-tray", 0)) == Some(1.) {
                    1.
                } else {
                    0.
                },
                crate::motion::PANEL,
            )
        } else {
            0.
        };
        // The header and the conversation arrive behind the frame's moving
        // edge. The composer does not: it is the pill the panel grew out
        // of, and it stays exactly as it was, at full strength, throughout.
        let arriving = if pose.value < 1. {
            crate::motion::content_fade(pose.value)
        } else {
            pose.fade
        };
        // Grown out of the focused bar's tray, the recent-chats button there
        // is the header's: it carries on rather than going out and coming
        // back, and rides the panel's top edge up.
        let leading = if pose.value < 1. {
            arriving.max(foot.min(1.))
        } else {
            arriving
        };
        let header = {
            // A hairline appears under it once the conversation has
            // scrolled beneath (see the body's fades).
            div()
                .relative()
                .h(px(48.))
                .flex_shrink_0()
                .pl(px(14.))
                .pr(px(13.5))
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    tool_tip("note-chat-history", "chat-history", "Recent chats", t)
                        .size(px(26.))
                        .opacity(leading)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_chat_history(cx))),
                )
                .child(div().opacity(leading).child(icon("chevron-down", 12., t.muted)))
                .child(div().flex_1())
                .child(
                    tool_tip("new-note-chat", "chat-new", "New chat", t)
                        .size(px(28.))
                        .opacity(arriving)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.clear_chat(cx);
                            this.ask.read(cx).focus_handle.focus(window);
                        })),
                )
                .child(
                    tool_tip("maximize-note-chat", "chat-maximize", "Maximize", t)
                        .size(px(28.))
                        .opacity(arriving)
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
        // The conversation dissolves into the panel at both ends instead of
        // running hard into the header or under the composer, and the
        // header gains its hairline while there is something above.
        let scrolled = self.stage.presence(
            ("chat-panel-rule", 0),
            self.chat_scroll.offset().y < px(-1.),
            crate::motion::SNAPPY,
        );
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("note-chat-scroll")
                    .track_scroll(&self.chat_scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                    .child(messages),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(18.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(t.card, 0.),
                        linear_color_stop(t.card.opacity(0.), 1.),
                    ))
                    .opacity(scrolled.fade),
            )
            .when(scrolled.visible, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(1.))
                        .bg(t.border.opacity(scrolled.fade)),
                )
            })
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(28.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(t.card.opacity(0.), 0.),
                        linear_color_stop(t.card, 1.),
                    )),
            );
        // The scroll-to-latest button and the history popover leave as they
        // arrive.
        let scroll_down = self.stage.presence(
            ("chat-scroll-down", 0),
            self.chat_can_scroll_down(),
            crate::motion::SNAPPY,
        );
        let history = self.stage.presence(
            ("chat-history", 0),
            self.chat_history_open,
            crate::motion::GOOEY,
        );
        let full = f32::from(height) + 2.;
        let pill = if home { crate::motion::ASK_PILL } else { NOTE_PILL };
        let unfold = crate::motion::ask_unfold(foot, pill, (f32::from(width), full));
        // Over a note: which note's record capsule shares the bottom row,
        // and how wide it is, for the bridge that joins it to the pill.
        let capsule = self
            .current_note
            .map(|id| (id, self.closed_capsule_width(id, cx)));
        let level = pose.value.max(0.);
        // How much of the panel the frame has reached so far, from its
        // bottom edge: the content is cut to it, as the frame's own would be.
        let reached = full * (unfold.scale.1 + (1. - unfold.scale.1) * level)
            - unfold.rise * (1. - level);
        let inner = div()
            .absolute()
            .bottom(px(0.))
            .w_full()
            .h(px(full))
            .rounded(px(crate::motion::ASK_FRAME_RADIUS))
            .p(px(1.))
            .flex()
            .flex_col()
            .overflow_hidden()
            // The header's room; the header itself rides the frame's edge
            // (below).
            .child(div().h(px(48.)).flex_shrink_0())
            .child(body.opacity(arriving))
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
                        .opacity(edit_banner.fade * arriving)
                        .px(px(4.))
                        .child(self.render_chat_edit_banner(cx)),
                )
            })
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(crate::motion::ASK_INSET - 1.))
                    .pb(px(crate::motion::ASK_INSET - 1.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    // Over a note the record capsule stays where it stood in
                    // the bar, beside the pill: the panel grows out of both.
                    .when_some(capsule, |row, (id, _)| {
                        row.child(self.render_recording_control(id, cx))
                    })
                    .child(
                        div().flex_1().min_w_0().child(
                            // No focus ring in the panel, even while its
                            // field has the keyboard: the panel is the
                            // frame, as in the reference.
                            self.render_chat_composer_focused(false, cx)
                                .when(!home, |pill| pill.min_h(px(NOTE_PILL))),
                        ),
                    ),
            )
            .when(scroll_down.visible, |d| {
                d.child(crate::motion::pop(
                    self.render_chat_scroll_down(cx),
                    crate::motion::Pop::chip((0.5, 1.)),
                    scroll_down,
                ))
            });
        // The frame (fill, hairline, shadow) is a shape of its own that
        // morphs out of the pill's outline; see `motion::Morph`. The content
        // lies over it, cut to as much of the panel as the frame has reached.
        let frame = crate::motion::pop(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .rounded(px(crate::motion::ASK_FRAME_RADIUS))
                .border_1()
                .border_color(t.border)
                .bg(t.card)
                .shadow(vec![BoxShadow {
                    // Soft and low: the panel reads by its hairline, with
                    // just enough under it to lift it off a note's text.
                    // It goes with the body: none left to pop off when the
                    // panel lands back in the bar.
                    color: hsla(0., 0., 0.1, if home { 0.06 } else { 0.14 } * level.min(1.)),
                    offset: point(px(0.), px(if home { 4. } else { 8. })),
                    blur_radius: px(if home { 16. } else { 28. }),
                    spread_radius: px(0.),
                }]),
            unfold,
            if crate::motion::reduce_motion() {
                pose
            } else {
                pose.solid()
            },
        );
        // On the frame's top edge all the way; out of the tray, 6 px higher
        // at first, where the tray had its button.
        let header_top = (full - reached).max(0.) + 1. - 6. * foot.min(1.) * (1. - level.min(1.));
        div()
            .absolute()
            // The pill stands where the bar's did: 27 px up on Home, 28 on
            // a note, with the inset under it.
            .bottom(px(if home { 23. } else { 28. - crate::motion::ASK_INSET }))
            .left((available_width - width) / 2. + px(if home { 0.5 } else { 0. }))
            .w(width)
            .h(px(full))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // A press outside the panel folds it back into the bar and gives
            // the focus back to the page; what was typed stays in the pill
            // and an answer that is arriving keeps arriving. The press goes
            // on to whatever it was aimed at. While the recent chats or the
            // model menu is open the press only closes that (each sees to
            // itself), as Escape does.
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left && !this.chat_popover_open() {
                    this.close_chat_surface(cx);
                    if this.ask.read(cx).focus_handle.is_focused(window) {
                        this.focus.focus(window);
                    }
                }
            }))
            .child(frame)
            .when_some(capsule, |panel, (_, capsule)| {
                panel.child(crate::motion::ask_bridge(
                    crate::motion::ASK_INSET + capsule + 4.,
                    pill,
                    reached,
                    level,
                    t.bg,
                ))
            })
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    // Below the header, which rides the frame's moving top
                    // edge: the conversation shows only under it, never
                    // behind it or past the frame's rounded corners. The
                    // bottom row (the pill, and a note's record capsule) is
                    // never cut: it is what the panel grows out of.
                    .h(px((reached - 49.)
                        .max(pill + crate::motion::ASK_INSET)
                        .clamp(0., full)))
                    .overflow_hidden()
                    .child(inner),
            )
            .child(
                div()
                    .absolute()
                    .left(px(1.))
                    .right(px(1.))
                    .top(px(header_top))
                    .child(header),
            )
            // The recent chats drop from the header's button, over the
            // conversation.
            .when(history.visible, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(header_top + 35.))
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
                        .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                            // Only the list closes: the field keeps the
                            // keyboard (as with the model menu).
                            if this.chat_history_open {
                                window.prevent_default();
                            }
                            this.dismiss_chat_history(cx)
                        })),
                )
            })
            .into_any_element()
    }
}

/// The Ask pill's height in a note's bar: the record capsule's.
const NOTE_PILL: f32 = 56.;

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

/// The `source` of the row that stands where a stretch of the recording has
/// no transcript.
pub(crate) const GAP_SOURCE: &str = "Transcript gap";

impl TranscriptUtterance {
    /// The row marks a stretch without transcript: it is not text anyone
    /// said, and is never found, cited or copied.
    pub(crate) fn is_gap(&self) -> bool {
        self.source == GAP_SOURCE
    }

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
    let rows = transcript_utterances(note);
    if let Some(segment_id) = segment_id {
        let id = crate::lazy::key(("segment", segment_id));
        return rows
            .iter()
            .position(|row| row.id == id && !row.is_gap() && row.text.contains(quote))
            .ok_or("This source quotation is no longer available.");
    }
    rows.iter()
        .position(|row| !row.is_gap() && row.text.contains(quote))
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
        // The passage glides to the middle of the panel and its ring plays
        // again, even when it is the one already open.
        crate::lazy::centre("transcript", target, transcript_glide_owner(note_id, self.transcript_epoch));
        self.stage.forget(SOURCE_RING);
        self.focus.focus(window);
        cx.notify();
    }
}

/// A note's transcript as the panel shows it: its rows in time order, with
/// a row "Transcript missing 12:03–12:10" where a stretch of the recording
/// was captured and never transcribed (`transcript_gaps`).
pub(crate) fn transcript_utterances(note: &crate::model::Note) -> Vec<TranscriptUtterance> {
    let (rows, starts): (Vec<_>, Vec<i64>) = if !note.server_transcript_segments.is_empty() {
        let starts = note
            .server_transcript_segments
            .iter()
            .filter(|segment| !segment.text.trim().is_empty())
            .map(|segment| segment.start_ms)
            .collect();
        (server_transcript_utterances(&note.server_transcript_segments), starts)
    } else {
        let rows = legacy_transcript_utterances(&note.transcript);
        let starts = rows
            .iter()
            .map(|row| {
                row.timestamp
                    .as_deref()
                    .and_then(crate::server::duration_seconds)
                    .map_or(i64::MAX, |seconds| (seconds * 1000.) as i64)
            })
            .collect();
        (rows, starts)
    };
    with_gap_rows(rows, &starts, &note.transcript_gaps)
}

/// `rows` (starting at `starts`, in milliseconds) with a row for each
/// stretch in `gaps` that is long enough to mention, at its time. Stretches
/// of the two channels that overlap are one row.
pub(crate) fn with_gap_rows(
    rows: Vec<TranscriptUtterance>,
    starts: &[i64],
    gaps: &[meeting_notes_api_types::TranscriptGapDTO],
) -> Vec<TranscriptUtterance> {
    let mut stretches: Vec<(i64, i64)> = gaps
        .iter()
        .filter(|gap| gap.to_ms - gap.from_ms >= crate::live_view::GAP_SHOWN_MS)
        .map(|gap| (gap.from_ms.max(0), gap.to_ms.max(0)))
        .collect();
    if stretches.is_empty() {
        return rows;
    }
    stretches.sort();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (from, to) in stretches {
        match merged.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    let gap_row = |(from, to): (i64, i64)| TranscriptUtterance {
        id: crate::lazy::key(("gap", from, to)),
        key: format!("gap:{from}"),
        speaker: None,
        part: None,
        own: false,
        source: GAP_SOURCE,
        timestamp: Some(crate::live_view::clock(from as u64)),
        text: format!(
            "Transcript missing {}",
            crate::live_view::gap_span(from as u64, to as u64)
        ),
        is_final: true,
    };
    let mut out = Vec::with_capacity(rows.len() + merged.len());
    let mut next = merged.into_iter().peekable();
    for (index, row) in rows.into_iter().enumerate() {
        let start = starts.get(index).copied().unwrap_or(i64::MAX);
        while let Some(gap) = next.next_if(|(from, _)| *from <= start) {
            out.push(gap_row(gap));
        }
        out.push(row);
    }
    out.extend(next.map(gap_row));
    out
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
        if utterance.is_gap() {
            continue;
        }
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

pub(crate) fn transcript_find_step(current: usize, count: usize, previous: bool) -> Option<usize> {
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
    let marks = transcript_marks(rows);
    (0..rows.len())
        .map(|index| {
            transcript_bubble(
                index,
                rows,
                voices,
                &marks,
                t,
                width,
                query,
                active_match,
                source_highlight,
                crate::motion::Pose::REST,
                1.,
                None,
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
            .filter(|row| !row.is_gap() && row.own == own && (row.is_final || row.speaker.is_some()))
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

/// The transcript's measurements, from the reference transcript panel (1 px
/// per pt captures of it): a 13 px line of 17.875 inside 8 × 6 padding, 8 px
/// corners that tighten to 6 where a bubble continues its voice, 2 px between
/// the bubbles of one voice and 10 px between voices.
const BUBBLE_TEXT: f32 = 13.;
const BUBBLE_LINE: f32 = 17.875;
const BUBBLE_PAD_X: f32 = 8.;
const BUBBLE_PAD_Y: f32 = 6.;
const BUBBLE_RADIUS: f32 = 8.;
const BUBBLE_JOINED_RADIUS: f32 = 6.;
/// Half the space between two rows of one voice (each row's own padding).
const ROW_PAD_Y: f32 = 1.;
/// What a row adds above and below itself where the voice changes.
const VOICE_GAP: f32 = 4.;
/// A speaker's name: 12/16 medium, 2 px above the bubble.
const LABEL_LINE: f32 = 16.;
const LABEL_GAP: f32 = 2.;
/// The centred time between voices: 8 px under the bubble above it, a 12/16
/// line, then the row's own 4 px.
const MARK_TOP: f32 = 8.;
const MARK_LINE: f32 = 16.;
/// The cited passage's outline, drawn on the bubble's own edge, and the
/// hairline of panel colour inside it.
const RING_WIDTH: f32 = 1.5;
const RING_GAP: f32 = 1.;
/// Which opening of which note's transcript a citation's glide belongs to:
/// the transcript list is one column for every note, so a glide that is
/// still under way ends when the panel closes, reopens or shows another note.
pub(crate) fn transcript_glide_owner(note_id: u64, epoch: u64) -> u64 {
    crate::lazy::key(("transcript-glide", note_id, epoch))
}
/// The cited passage's outline: it fades in once per citation.
pub(crate) const SOURCE_RING: crate::motion::TrackKey = ("transcript-source-ring", 0);
/// How far a 12 px time sits above a 13 px line's baseline in the same
/// line box, so it is moved down by it and the two baselines agree. Measured
/// on 1 px per pt captures (the stems' bottom edges, from their coverage):
/// 1.08 px, of which the font's metrics (ascent 0.986 em, descent 0.327 em)
/// account for 0.33 and the rest is where text is placed on the pixel grid.
const TIME_BASELINE: f32 = 1.;
/// Which rows are preceded by a centred time. None are: the reference
/// panel has no times between the bubbles, and they read as unexplained
/// numbers. (The cited passage still shows its own time beside it.) Kept as
/// a per-row answer so the row layout and its height estimate stay one rule.
fn transcript_marks(rows: Rows) -> Vec<bool> {
    vec![false; rows.len()]
}

/// Everything that decides how tall utterance `index` is drawn (at a given
/// width): its text, its spacing from its neighbours, whether it is named and
/// whether a centred time precedes it. A bubble is measured once per key; the
/// source ring is drawn outside the bubble and does not change its height.
fn transcript_key(index: usize, rows: Rows, voices: (usize, usize), marks: &[bool]) -> u64 {
    // Not the index itself: a row keeps its key, and so its measured height,
    // when rows are inserted above it.
    crate::lazy::key((
        index == 0,
        &rows.at(index).text,
        transcript_neighbours(index, rows),
        index + 1 == rows.len(),
        transcript_names_speaker(index, rows, voices),
        marks.get(index).copied().unwrap_or(false),
    ))
}

/// The space a row leaves above its stack (inside and outside the row):
/// the voice gap or the centred time, its padding, and the panel's top inset.
fn transcript_row_top(index: usize, same_before: bool, marked: bool) -> (f32, f32) {
    let margin = if marked {
        MARK_TOP
    } else if !same_before {
        VOICE_GAP
    } else {
        0.
    } + if index == 0 { 6. } else { 0. };
    // After a mark the row keeps the voice gap of its own, under the time.
    let inside = ROW_PAD_Y + if marked { VOICE_GAP } else { 0. };
    (margin, inside)
}

/// A guess of how tall [`transcript_bubble`] will draw row `index`, from the
/// same numbers it lays out with and a quick count of the text's lines. Rows
/// that have not been on screen stand in the scroll range at this height.
fn transcript_row_estimate(
    index: usize,
    rows: Rows,
    voices: (usize, usize),
    width: Pixels,
    marks: &[bool],
    counter: &mut LineCounter,
) -> f32 {
    if rows.at(index).is_gap() {
        return GAP_ROW_HEIGHT
            + if index == 0 { 6. } else { 0. }
            + if index + 1 == rows.len() { 6. } else { 0. };
    }
    let (same_before, same_after) = transcript_neighbours(index, rows);
    let marked = marks.get(index).copied().unwrap_or(false);
    // The bubble's column, less the bubble's padding.
    let column = (f32::from(width) - 80.).max(1.);
    let text_width = column - 2. * BUBBLE_PAD_X;
    let lines = counter.lines(&rows.at(index).text, text_width) as f32;
    let label = if transcript_names_speaker(index, rows, voices) {
        LABEL_LINE + LABEL_GAP
    } else {
        0.
    };
    let mark = if marked { MARK_LINE } else { 0. };
    let (top, inside) = transcript_row_top(index, same_before, marked);
    let bottom = if same_after { 0. } else { VOICE_GAP }
        + if index + 1 == rows.len() { 6. } else { 0. };
    lines * BUBBLE_LINE + 2. * BUBBLE_PAD_Y + label + mark + top + inside + ROW_PAD_Y + bottom
}

/// How tall a gap row is drawn: its 20 px line and 4 px either side.
const GAP_ROW_HEIGHT: f32 = 28.;

/// The small centred row where a stretch of the recording has no
/// transcript.
fn transcript_gap_row(index: usize, rows: Rows, t: Theme, width: Pixels) -> AnyElement {
    let row = rows.at(index);
    div()
        .id(("transcript-gap", index))
        .w(width - px(16.))
        .mx(px(12.))
        .flex_shrink_0()
        .mt(px(4. + if index == 0 { 6. } else { 0. }))
        .mb(px(4. + if index + 1 == rows.len() { 6. } else { 0. }))
        .h(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(5.))
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(t.muted)
        .child(icon("warning", 11., t.muted))
        .child(row.text.clone())
        .into_any_element()
}

/// What a row that is still being spoken (or has just become final) needs
/// besides its text.
pub(crate) struct LiveLine {
    /// `None` while spoken; then how far the final has settled, 0..=1.
    settle: Option<f32>,
    /// The interim row this final took over from, and the words it showed.
    inherits: Option<u64>,
    ghost: Option<SharedString>,
}

/// One utterance of the transcript.
#[allow(clippy::too_many_arguments)]
fn transcript_bubble(
    index: usize,
    rows: Rows,
    voices: (usize, usize),
    marks: &[bool],
    t: Theme,
    width: Pixels,
    query: &str,
    active_match: Option<&TranscriptFindMatch>,
    source_highlight: Option<usize>,
    // Where the source ring is in its entrance (see [`SOURCE_RING`]).
    ring: crate::motion::Pose,
    // How far the row is into its entrance (1 at rest).
    level: f32,
    // Set for a line still being spoken or just become final.
    live: Option<&LiveLine>,
) -> AnyElement {
    let dark = t.bg.l < 0.5;
    if rows.at(index).is_gap() {
        return transcript_gap_row(index, rows, t, width);
    }
    let utterance = rows.at(index);
    let (same_before, same_after) = transcript_neighbours(index, rows);
    let marked = marks.get(index).copied().unwrap_or(false);
    let cited = source_highlight == Some(index);
    let own = utterance.own;
    // Every match on a yellow box, the one Find is on in orange, dark text
    // on both, boxed line by line where a match wraps.
    let marks: Vec<_> = if live.is_some() {
        Vec::new()
    } else {
        transcript_match_ranges(&utterance.text, query)
            .into_iter()
            .map(|range| {
                let current = active_match.is_some_and(|active| {
                    active.utterance_index == index && active.text_range.as_ref() == Some(&range)
                });
                (range, current)
            })
            .collect()
    };
    let text = crate::transcript_panel::marked_text(utterance.text.clone(), marks);
    // Corners: 8 px, tightened to 6 on the side that continues the voice.
    let corner = |joined: bool| if joined { BUBBLE_JOINED_RADIUS } else { BUBBLE_RADIUS };
    let (top_left, top_right, bottom_left, bottom_right) = (
        corner(same_before && !own),
        corner(same_before && own),
        corner(same_after && !own),
        corner(same_after && own),
    );
    let fill: Hsla = if dark {
        if own { rgb(0x292929) } else { rgb(0x4e4d4b) }
    } else if own {
        rgba(0xe0af2f66)
    } else {
        rgb(0xf2f2ec)
    }
    .into();
    let ink: Hsla = if dark {
        if own { rgb(0xfcf2df) } else { rgb(0xfcfcf8) }
    } else if own {
        rgb(0x363024)
    } else {
        rgb(0x292929)
    }
    .into();
    let mut bubble = div()
        .relative()
        .max_w_full()
        .min_w_0()
        .px(px(BUBBLE_PAD_X))
        .py(px(BUBBLE_PAD_Y))
        .rounded_tl(px(top_left))
        .rounded_tr(px(top_right))
        .rounded_bl(px(bottom_left))
        .rounded_br(px(bottom_right))
        .text_size(px(BUBBLE_TEXT))
        .line_height(px(BUBBLE_LINE))
        .bg(fill)
        .text_color(ink)
        .when(level < 1., |d| d.opacity(level))
        .child(text);
    if cited {
        // An outline on the bubble's own edge, in the gold of the app's
        // accent, then a hairline of the panel's colour between it and the
        // fill: it reads as a selection rather than a thicker bubble, keeps
        // the 2 pt gap to the bubbles around it and stays inside the
        // column. It only fades in; nothing moves.
        let ink = Hsla::from(rgb(0xe0af2f)).opacity(if dark { 0.85 } else { 0.9 } * ring.fade);
        let inner = RING_WIDTH;
        bubble = bubble
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_tl(px(top_left))
                    .rounded_tr(px(top_right))
                    .rounded_bl(px(bottom_left))
                    .rounded_br(px(bottom_right))
                    .border(px(RING_WIDTH))
                    .border_color(ink),
            )
            .child(
                div()
                    .absolute()
                    .top(px(inner))
                    .left(px(inner))
                    .right(px(inner))
                    .bottom(px(inner))
                    .rounded_tl(px(top_left - inner))
                    .rounded_tr(px(top_right - inner))
                    .rounded_bl(px(bottom_left - inner))
                    .rounded_br(px(bottom_right - inner))
                    .border(px(RING_GAP))
                    .border_color(t.card.opacity(ring.fade)),
            );
        // Its time beside it, on its first line's box so the two baselines
        // agree. Only the cited bubble shows one, as in the reference.
        if let Some(timestamp) = &utterance.timestamp {
            bubble = bubble.child(
                div()
                    .absolute()
                    .top(px(BUBBLE_PAD_Y + TIME_BASELINE))
                    .text_size(px(12.))
                    .line_height(px(BUBBLE_LINE))
                    .whitespace_nowrap()
                    .text_color(t.muted)
                    .opacity(ring.fade)
                    .when(own, |d| d.right(relative(1.)).mr(px(8.)))
                    .when(!own, |d| d.left(relative(1.)).ml(px(8.)))
                    .child(timestamp.clone()),
            );
        }
    }
    let mut stack = div()
        // Give the column a definite wrapping width. Percentage-only
        // max-width on nested flex columns can measure the text at zero
        // width, leaving the row's hitbox but no painted bubble.
        .w((width - px(80.)).max(px(1.)))
        .min_w_0()
        .max_w_full()
        .flex()
        .flex_col()
        .gap(px(LABEL_GAP))
        .when(own, |d| d.items_end())
        .when(!own, |d| d.items_start());
    if transcript_names_speaker(index, rows, voices)
        && let Some(speaker) = &utterance.speaker
    {
        stack = stack.child(
            div()
                .px(px(4.))
                .text_size(px(12.))
                .line_height(px(LABEL_LINE))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.muted)
                .child(speaker.clone()),
        );
    }
    // A line still being spoken, or just become final, is the live bubble:
    // it eases to its text's size, its dots pulse, its final settles in it.
    let bubble = match live {
        Some(line) => {
            let bubble = crate::transcript_panel::live_bubble(
                utterance.id,
                line.inherits,
                utterance.text.clone(),
                line.ghost.clone(),
                own,
                fill,
                ink,
                [top_left, top_right, bottom_right, bottom_left],
                line.settle,
                line.settle.is_none(),
            )
            .metrics((BUBBLE_PAD_X, BUBBLE_PAD_Y), (BUBBLE_TEXT, BUBBLE_LINE));
            if line.settle.is_none() {
                crate::motion::slow_loop(
                    ("live-dots", utterance.id),
                    crate::transcript_panel::DOTS_PERIOD,
                    bubble,
                    |bubble, turns| bubble.turns(turns),
                )
                .into_any_element()
            } else {
                bubble.into_any_element()
            }
        }
        None => bubble.into_any_element(),
    };
    stack = stack
        .when(live.is_some() && level < 1., |stack| stack.opacity(level))
        .child(bubble);
    let (top, inside) = transcript_row_top(index, same_before, marked);
    let row = div()
        .id(("transcript-bubble", index))
        .w_full()
        .min_w_0()
        .flex()
        .pt(px(inside))
        .pb(px(ROW_PAD_Y))
        .when(own, |d| d.pl(px(64.)).justify_end())
        .when(!own, |d| d.pr(px(64.)).justify_start())
        .child(stack);
    div()
        .w(width - px(16.))
        .min_w_0()
        .flex_shrink_0()
        .whitespace_normal()
        .flex()
        .flex_col()
        .mx(px(12.))
        .mt(px(top))
        .mb(px(if !same_after { VOICE_GAP } else { 0. }
            + if index + 1 == rows.len() { 6. } else { 0. }))
        .when(marked, |column| {
            column.child(
                div()
                    .h(px(MARK_LINE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(MARK_LINE))
                    .text_color(t.muted)
                    // A new row's time fades in with its words and is then
                    // at full strength, while they are spoken and after.
                    .when(!utterance.is_final && level < 0.5, |d| d.opacity(level * 2.))
                    .children(utterance.timestamp.clone()),
            )
        })
        .child(row)
        .into_any_element()
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
    use super::{
        GLYPH_TAB, NOTES_LEAD, SPARKLE_LEAD_COLLAPSED, SPARKLE_TRAIL_COLLAPSED, metadata_anchors,
        note_date_label, note_tab_widths,
    };
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
    fn popovers_follow_the_joined_views() {
        let enhanced = metadata_anchors(1200., Some(147.));
        assert_eq!(enhanced.details_left, 443.);
        let ungenerated = metadata_anchors(1200., None);
        assert_eq!(ungenerated.details_left, 293.);
    }

    #[test]
    fn collapsed_view_tabs_fit_their_glyphs() {
        // "My notes" and "Enhanced" as shaped at 13 px on the note page: the
        // reference's segments (35 | 113 and 98 | 50) less the point each
        // label is narrower in this face, and one track width in both states.
        let own = note_tab_widths(52.611, 58.981, true);
        assert_eq!(
            (
                own.notes_collapsed,
                own.enhanced,
                own.notes,
                own.enhanced_collapsed
            ),
            (35., 112., 97., 50.)
        );
        assert_eq!(own.track(NoteTab::Notes), 147.);
        assert_eq!(own.track(NoteTab::Enhanced), 147.);
        // The shared view ("Notes", no template chevron): each collapsed tab
        // is only its glyph, whatever the other label's length.
        for notes_label in [33.4, 36., 90.] {
            let shared = note_tab_widths(notes_label, 58.981, false);
            assert_eq!(shared.notes_collapsed, GLYPH_TAB);
            assert_eq!(shared.enhanced_collapsed, 39.);
            assert!(shared.notes > shared.notes_collapsed);
            assert!(shared.enhanced > shared.enhanced_collapsed);
        }
        // Collapsed glyphs keep the inset the lines have from the track's
        // start: 13.5 pt of ink (the glyph boxes carry about 2.3 pt of
        // their own margin at the start and 2 pt at the end).
        let lines_start = NOTES_LEAD + 2.4;
        let sparkle_end = SPARKLE_TRAIL_COLLAPSED + 2.1;
        assert!((lines_start - 13.5).abs() < 0.3, "{lines_start}");
        assert!((sparkle_end - 13.5).abs() < 0.3, "{sparkle_end}");
        // And the collapsed sparkle sits near the middle of its tab.
        let sparkle_start = SPARKLE_LEAD_COLLAPSED + 1.0;
        assert!((sparkle_start - sparkle_end).abs() < 1.5);
    }

    #[test]
    fn popovers_track_resizing_and_stay_within_compact_window_edges() {
        let wide = metadata_anchors(1200., Some(147.));
        let compact = metadata_anchors(760., Some(147.));
        assert_eq!(wide.details_left - compact.details_left, 220.);
        let narrow = metadata_anchors(500., Some(147.));
        assert!(narrow.details_left >= 12. && narrow.details_left + 280. <= 488.);
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::{
        Rows, transcript_content, transcript_find_matches, transcript_find_step,
        transcript_match_ranges, transcript_quote_target, transcript_utterances, transcript_voices,
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
    fn the_live_panel_has_no_centred_times_between_bubbles() {
        // A row every 20 s, the channel changing every second row: before,
        // a time was set between the bubbles at most once a minute.
        let stored: Vec<_> = (0..12)
            .map(|index| {
                let mut row = row(&format!("Line {index}."));
                row.own = index / 2 % 2 == 1;
                row.key = format!("{}", row.own);
                row.timestamp = Some(format!("{:02}:{:02}", index * 20 / 60, index * 20 % 60));
                row
            })
            .collect();
        let speaking = [row("and then")];
        let rows = super::Rows {
            stored: &stored,
            live: &speaking,
        };
        assert!(super::transcript_marks(rows).iter().all(|marked| !marked));
        assert_eq!(super::transcript_marks(rows).len(), 13);
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
        let key = super::transcript_key(2, rows, voices, &[]);
        assert_eq!(key, super::transcript_key(2, rows, voices, &[]));
        let mut longer = speaking.clone();
        longer[0].text.push_str("ee");
        let grown = super::Rows {
            stored: &stored,
            live: &longer,
        };
        assert_ne!(key, super::transcript_key(2, grown, voices, &[]));
        // Stored rows are not re-keyed by interim text changing below them,
        // except the last one, whose spacing depends on what follows.
        assert_eq!(
            super::transcript_key(0, rows, voices, &[]),
            super::transcript_key(0, grown, voices, &[])
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

    #[test]
    fn stretches_without_transcript_are_rows_at_their_time() {
        use meeting_notes_api_types::TranscriptGapDTO;
        let spoken = |id: &str, start_ms: i64, text: &str| TranscriptSegmentDTO {
            id: id.into(),
            channel: AudioChannel::System,
            speaker_id: "system:n:1".into(),
            speaker_label: Some("Speaker 1".into()),
            text: text.into(),
            is_final: true,
            start_ms,
            end_ms: Some(start_ms + 2_000),
        };
        let gap = |channel, from_ms, to_ms| TranscriptGapDTO { channel, from_ms, to_ms };
        let note = Note {
            server_transcript_segments: vec![
                spoken("a", 0, "Good morning."),
                spoken("b", 10_000, "Shall we start?"),
                spoken("c", 725_000, "Where were we."),
            ],
            transcript_gaps: vec![
                // The network went: both channels, one row.
                gap(AudioChannel::Mic, 723_000, 730_000),
                gap(AudioChannel::System, 723_500, 729_000),
                // Too short to mention.
                gap(AudioChannel::Mic, 20_000, 20_400),
                // After the last words.
                gap(AudioChannel::System, 800_000, 860_000),
            ],
            ..Note::default()
        };
        let rows = transcript_utterances(&note);
        let shown: Vec<_> = rows.iter().map(|row| (row.is_gap(), row.text.as_str())).collect();
        assert_eq!(
            shown,
            [
                (false, "Good morning."),
                (false, "Shall we start?"),
                (true, "Transcript missing 12:03–12:10"),
                (false, "Where were we."),
                (true, "Transcript missing 13:20–14:20"),
            ]
        );
        assert_eq!(rows[2].timestamp.as_deref(), Some("12:03"));
        // A gap row is not text anyone said: not a voice, not found, not cited.
        assert_eq!(transcript_voices(Rows { stored: &rows, live: &[] }), (0, 1));
        assert!(transcript_find_matches(&rows, "missing").is_empty());
        assert_eq!(transcript_quote_target(&note, Some("c"), "Where", None), Ok(3));
        // Drawn as rows of their own.
        let rendered = transcript_content(&rows, crate::ui::Theme::new(true), gpui::px(674.), "", None, None);
        assert_eq!(rendered.len(), 5);
        // A note without gaps is exactly its segments.
        let plain = Note { transcript_gaps: Vec::new(), ..note };
        assert_eq!(transcript_utterances(&plain).len(), 3);
    }
}
