use crate::domain::TranslatedDocument;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub type ReviewResponder =
    Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<Option<TranslatedDocument>>>>>;
pub type ActionCallback = Rc<RefCell<Option<Box<dyn Fn() + 'static>>>>;

#[derive(Clone)]
pub struct ReviewTranscriptView {
    scroller: gtk4::ScrolledWindow,
    segments_box: gtk4::Box,
    confirm_btn: gtk4::Button,
    cancel_btn: gtk4::Button,
    current_doc: Rc<RefCell<Option<TranslatedDocument>>>,
    entry_rows: Rc<RefCell<Vec<(String, libadwaita::EntryRow)>>>,
    current_responder: Rc<RefCell<Option<ReviewResponder>>>,
    on_proceed: ActionCallback,
    on_cancelled: ActionCallback,
}

impl Default for ReviewTranscriptView {
    fn default() -> Self {
        Self::new()
    }
}

impl ReviewTranscriptView {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_margin_top(24);
        container.set_margin_bottom(24);
        container.set_margin_start(32);
        container.set_margin_end(32);

        // Header
        let title = gtk4::Label::new(Some("Review & Edit Translation"));
        title.add_css_class("title-1");

        let subtitle = gtk4::Label::new(Some(
            "Fine-tune terms, names, or phrasing before synthesizing audio. Changes directly apply to the dubbed voices.",
        ));
        subtitle.add_css_class("dim-label");
        subtitle.set_wrap(true);

        container.append(&title);
        container.append(&subtitle);

        // Scrollable segments list
        let segments_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        segments_box.set_vexpand(true);
        container.append(&segments_box);

        // Action Buttons Row
        let actions_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        actions_box.set_halign(gtk4::Align::End);
        actions_box.set_margin_top(16);

        let cancel_btn = gtk4::Button::with_label("Cancel Dubbing");
        cancel_btn.add_css_class("flat");

        let confirm_btn = gtk4::Button::with_label("Proceed to Dubbing");
        confirm_btn.add_css_class("suggested-action");
        confirm_btn.add_css_class("pill");

        actions_box.append(&cancel_btn);
        actions_box.append(&confirm_btn);
        container.append(&actions_box);

        let clamp = libadwaita::Clamp::new();
        clamp.set_maximum_size(840);
        clamp.set_tightening_threshold(640);
        clamp.set_child(Some(&container));

        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scroller.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);
        scroller.set_child(Some(&clamp));

        let current_doc = Rc::new(RefCell::new(None));
        let entry_rows = Rc::new(RefCell::new(Vec::new()));
        let current_responder = Rc::new(RefCell::new(None));
        let on_proceed = Rc::new(RefCell::new(None));
        let on_cancelled = Rc::new(RefCell::new(None));

        let view = Self {
            scroller,
            segments_box,
            confirm_btn,
            cancel_btn,
            current_doc,
            entry_rows,
            current_responder,
            on_proceed,
            on_cancelled,
        };

        // Wire up buttons
        let view_clone = view.clone();
        view.confirm_btn.connect_clicked(move |_| {
            view_clone.handle_confirm();
        });

        let view_clone = view.clone();
        view.cancel_btn.connect_clicked(move |_| {
            view_clone.handle_cancel();
        });

        view
    }

    pub fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scroller
    }

    pub fn connect_proceed<F: Fn() + 'static>(&self, callback: F) {
        *self.on_proceed.borrow_mut() = Some(Box::new(callback));
    }

    pub fn connect_cancelled<F: Fn() + 'static>(&self, callback: F) {
        *self.on_cancelled.borrow_mut() = Some(Box::new(callback));
    }

    pub fn populate(&self, doc: TranslatedDocument, responder: ReviewResponder) {
        *self.current_doc.borrow_mut() = Some(doc.clone());
        *self.current_responder.borrow_mut() = Some(responder);

        // Clear previous rows
        while let Some(child) = self.segments_box.first_child() {
            self.segments_box.remove(&child);
        }
        self.entry_rows.borrow_mut().clear();

        for (idx, seg) in doc.segments.iter().enumerate() {
            let pref_group = libadwaita::PreferencesGroup::new();
            let spk = seg.speaker_id.as_deref().unwrap_or("Speaker 1");
            let time_str = format!(
                "{} ➔ {}",
                format_ms(seg.source_start_ms),
                format_ms(seg.source_end_ms)
            );
            pref_group.set_title(&format!("Segment #{} • {} ({})", idx + 1, spk, time_str));
            pref_group.set_description(Some(&format!("Original: \"{}\"", seg.source_text)));

            let entry_row = libadwaita::EntryRow::new();
            entry_row.set_title("Translation");
            entry_row.set_text(&seg.translated_text);

            pref_group.add(&entry_row);
            self.segments_box.append(&pref_group);

            self.entry_rows
                .borrow_mut()
                .push((seg.segment_id.clone(), entry_row));
        }
    }

    fn handle_confirm(&self) {
        if let Some(mut doc) = self.current_doc.borrow().clone() {
            let rows = self.entry_rows.borrow();
            let text_map: std::collections::HashMap<String, String> = rows
                .iter()
                .map(|(id, row)| (id.clone(), row.text().to_string()))
                .collect();

            for seg in &mut doc.segments {
                if let Some(new_text) = text_map.get(&seg.segment_id) {
                    seg.translated_text = new_text.clone();
                }
            }

            if let Some(resp) = self.current_responder.borrow_mut().take() {
                glib::MainContext::default().spawn_local(async move {
                    if let Some(sender) = resp.lock().await.take() {
                        let _ = sender.send(Some(doc));
                    }
                });
            }

            if let Some(ref cb) = *self.on_proceed.borrow() {
                cb();
            }
        }
    }

    fn handle_cancel(&self) {
        if let Some(resp) = self.current_responder.borrow_mut().take() {
            glib::MainContext::default().spawn_local(async move {
                if let Some(sender) = resp.lock().await.take() {
                    let _ = sender.send(None);
                }
            });
        }

        if let Some(ref cb) = *self.on_cancelled.borrow() {
            cb();
        }
    }
}

fn format_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let millis = ms % 1000;
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}
