use crate::application::ports::JobRepository;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::sync::Arc;

#[derive(Clone)]
pub struct HistoryView {
    container: gtk4::Box,
    list_box: gtk4::ListBox,
    back_btn: gtk4::Button,
}

impl Default for HistoryView {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryView {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_margin_top(24);
        container.set_margin_bottom(24);
        container.set_margin_start(24);
        container.set_margin_end(24);

        let top_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);

        let back_btn = gtk4::Button::from_icon_name("go-previous-symbolic");
        back_btn.set_tooltip_text(Some("Back to Translate"));
        back_btn.add_css_class("flat");

        let title = gtk4::Label::new(Some("Translation History"));
        title.add_css_class("title-2");
        title.set_halign(gtk4::Align::Start);
        title.set_hexpand(true);

        top_box.append(&back_btn);
        top_box.append(&title);
        container.append(&top_box);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_vexpand(true);

        let list_box = gtk4::ListBox::new();
        list_box.add_css_class("boxed-list");
        scrolled.set_child(Some(&list_box));

        container.append(&scrolled);

        Self {
            container,
            list_box,
            back_btn,
        }
    }

    pub fn connect_back_clicked<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.back_btn.connect_clicked(move |_| callback());
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    pub async fn refresh(&self, repo: Arc<dyn JobRepository>) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        if let Ok(jobs) = repo.list().await {
            if jobs.is_empty() {
                let empty_row = libadwaita::ActionRow::new();
                empty_row.set_title("No previous translation jobs");
                empty_row.set_subtitle("Completed audio translations will appear here.");
                self.list_box.append(&empty_row);
            } else {
                for job in jobs {
                    let row = libadwaita::ActionRow::new();
                    let file_name = job.source_audio.file_name();
                    row.set_title(&format!(
                        "{} ({} → {})",
                        file_name, job.source_language, job.target_language
                    ));
                    row.set_subtitle(&format!(
                        "Status: {} · Created: {}",
                        job.stage.display_label(),
                        job.created_at.format("%Y-%m-%d %H:%M")
                    ));
                    self.list_box.append(&row);
                }
            }
        }
    }
}
