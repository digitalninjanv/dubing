use crate::domain::AudioArtifact;
use gtk4::prelude::*;
use std::path::PathBuf;

#[derive(Clone)]
pub struct ResultView {
    container: gtk4::Box,
    info_label: gtk4::Label,
    path_label: gtk4::Label,
    warning_box: gtk4::Box,
    warning_label: gtk4::Label,
    #[allow(dead_code)]
    play_btn: gtk4::Button,
    #[allow(dead_code)]
    open_folder_btn: gtk4::Button,
    new_btn: gtk4::Button,
    current_output_path: std::rc::Rc<std::cell::RefCell<Option<PathBuf>>>,
}

impl Default for ResultView {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultView {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_margin_top(32);
        container.set_margin_bottom(32);
        container.set_margin_start(32);
        container.set_margin_end(32);

        let icon = gtk4::Image::from_icon_name("emblem-ok-symbolic");
        icon.set_pixel_size(64);
        icon.add_css_class("success");
        container.append(&icon);

        let title = gtk4::Label::new(Some("Audio Dubbing Complete!"));
        title.add_css_class("title-1");
        container.append(&title);

        let info_frame = gtk4::Frame::new(None);
        info_frame.add_css_class("card");
        info_frame.set_margin_top(16);

        let info_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        info_box.set_margin_top(16);
        info_box.set_margin_bottom(16);
        info_box.set_margin_start(16);
        info_box.set_margin_end(16);

        let info_label = gtk4::Label::new(None);
        info_label.set_wrap(true);
        info_label.add_css_class("heading");

        let path_label = gtk4::Label::new(None);
        path_label.set_wrap(true);
        path_label.add_css_class("dim-label");

        let warning_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        warning_box.add_css_class("card");
        warning_box.set_margin_top(8);
        warning_box.set_visible(false);

        let warning_title = gtk4::Label::new(Some("⚠ Quality Notice"));
        warning_title.add_css_class("warning");
        warning_title.set_halign(gtk4::Align::Start);

        let warning_label = gtk4::Label::new(None);
        warning_label.set_wrap(true);
        warning_label.set_halign(gtk4::Align::Start);
        warning_label.add_css_class("dim-label");

        warning_box.append(&warning_title);
        warning_box.append(&warning_label);

        info_box.append(&info_label);
        info_box.append(&path_label);
        info_box.append(&warning_box);
        info_frame.set_child(Some(&info_box));
        container.append(&info_frame);

        // Action Buttons
        let actions_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        actions_box.set_halign(gtk4::Align::Center);
        actions_box.set_margin_top(24);

        let play_btn = gtk4::Button::with_label("▶ Play Audio");
        play_btn.add_css_class("suggested-action");
        play_btn.add_css_class("pill");

        let open_folder_btn = gtk4::Button::with_label("Open Containing Folder");
        open_folder_btn.add_css_class("pill");

        actions_box.append(&play_btn);
        actions_box.append(&open_folder_btn);
        container.append(&actions_box);

        let new_btn = gtk4::Button::with_label("Start Another Translation");
        new_btn.add_css_class("pill");
        new_btn.set_halign(gtk4::Align::Center);
        new_btn.set_margin_top(16);
        container.append(&new_btn);

        let current_output_path = std::rc::Rc::new(std::cell::RefCell::new(None::<PathBuf>));

        // Connect open folder action
        let path_clone = current_output_path.clone();
        open_folder_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_clone.borrow() {
                if let Some(parent) = path.parent() {
                    let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                }
            }
        });

        // Connect play audio action
        let path_clone2 = current_output_path.clone();
        play_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_clone2.borrow() {
                let _ = std::process::Command::new("xdg-open").arg(path).spawn();
            }
        });

        Self {
            container,
            info_label,
            path_label,
            warning_box,
            warning_label,
            play_btn,
            open_folder_btn,
            new_btn,
            current_output_path,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    pub fn set_result(&self, artifact: &AudioArtifact, source_lang: &str, target_lang: &str) {
        *self.current_output_path.borrow_mut() = Some(artifact.path.clone());

        let mins = artifact.duration_ms / 60000;
        let secs = (artifact.duration_ms % 60000) / 1000;
        let size_mb = (artifact.size_bytes as f64) / (1024.0 * 1024.0);

        self.info_label.set_text(&format!(
            "Translated from {} → {}\nDuration: {:02}:{:02} · File Size: {:.2} MB",
            source_lang, target_lang, mins, secs, size_mb
        ));

        self.path_label
            .set_text(&format!("Saved to: {}", artifact.path.display()));

        if !artifact.quality_warnings.is_empty() {
            self.warning_label
                .set_text(&artifact.quality_warnings.join("\n"));
            self.warning_box.set_visible(true);
        } else {
            self.warning_box.set_visible(false);
        }
    }

    pub fn connect_new_clicked<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.new_btn.connect_clicked(move |_| callback());
    }
}
