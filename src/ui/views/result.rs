use crate::domain::AudioArtifact;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone)]
pub struct ResultView {
    scroller: gtk4::ScrolledWindow,
    info_label: gtk4::Label,
    path_label: gtk4::Label,
    warning_box: gtk4::Box,
    warning_label: gtk4::Label,
    #[allow(dead_code)]
    play_btn: gtk4::Button,
    play_video_btn: gtk4::Button,
    subtitles_row: libadwaita::ActionRow,
    script_row: libadwaita::ActionRow,
    new_btn: gtk4::Button,
    current_output_path: Rc<RefCell<Option<PathBuf>>>,
    current_video_path: Rc<RefCell<Option<PathBuf>>>,
    current_subtitle_path: Rc<RefCell<Option<PathBuf>>>,
    current_txt_path: Rc<RefCell<Option<PathBuf>>>,
}

impl Default for ResultView {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultView {
    pub fn new() -> Self {
        // Root Scrolled Window to allow full responsiveness and prevent window size locking
        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scroller.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
        scroller.set_propagate_natural_width(false);
        scroller.set_propagate_natural_height(false);
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);

        // Libadwaita Clamp keeps content centered and beautifully sized on all resolutions
        let clamp = libadwaita::Clamp::new();
        clamp.set_maximum_size(780);
        clamp.set_tightening_threshold(580);
        clamp.set_vexpand(true);

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        content_box.set_margin_top(24);
        content_box.set_margin_bottom(32);
        content_box.set_margin_start(16);
        content_box.set_margin_end(16);

        // Header / Success Banner
        let icon = gtk4::Image::from_icon_name("emblem-ok-symbolic");
        icon.set_pixel_size(56);
        icon.add_css_class("success");
        icon.set_margin_top(8);
        content_box.append(&icon);

        let title = gtk4::Label::new(Some("Dubbing & Translation Complete!"));
        title.add_css_class("title-1");
        title.set_justify(gtk4::Justification::Center);
        content_box.append(&title);

        // Summary Card
        let info_frame = gtk4::Frame::new(None);
        info_frame.add_css_class("card");
        info_frame.set_margin_top(8);

        let info_box = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        info_box.set_margin_top(16);
        info_box.set_margin_bottom(16);
        info_box.set_margin_start(16);
        info_box.set_margin_end(16);

        let info_label = gtk4::Label::new(None);
        info_label.set_wrap(true);
        info_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        info_label.set_justify(gtk4::Justification::Center);
        info_label.add_css_class("heading");

        let path_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        path_box.set_halign(gtk4::Align::Center);
        path_box.set_hexpand(true);

        let path_label = gtk4::Label::new(None);
        path_label.set_wrap(true);
        path_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        path_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        path_label.set_max_width_chars(60);
        path_label.add_css_class("dim-label");

        let copy_path_btn = gtk4::Button::from_icon_name("edit-copy-symbolic");
        copy_path_btn.add_css_class("flat");
        copy_path_btn.set_tooltip_text(Some("Copy output path"));

        path_box.append(&path_label);
        path_box.append(&copy_path_btn);

        let warning_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        warning_box.add_css_class("card");
        warning_box.set_margin_top(6);
        warning_box.set_visible(false);

        let warning_title = gtk4::Label::new(Some("⚠ Quality Notice"));
        warning_title.add_css_class("warning");
        warning_title.set_halign(gtk4::Align::Start);

        let warning_label = gtk4::Label::new(None);
        warning_label.set_wrap(true);
        warning_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        warning_label.set_halign(gtk4::Align::Start);
        warning_label.add_css_class("dim-label");

        warning_box.append(&warning_title);
        warning_box.append(&warning_label);

        info_box.append(&info_label);
        info_box.append(&path_box);
        info_box.append(&warning_box);
        info_frame.set_child(Some(&info_box));
        content_box.append(&info_frame);

        // Prominent Playback Buttons
        let play_actions_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        play_actions_box.set_halign(gtk4::Align::Center);
        play_actions_box.set_margin_top(12);

        let play_btn = gtk4::Button::with_label("▶ Play Dubbed Audio");
        play_btn.add_css_class("suggested-action");
        play_btn.add_css_class("pill");

        let play_video_btn = gtk4::Button::with_label("🎬 Play Dubbed Video");
        play_video_btn.add_css_class("suggested-action");
        play_video_btn.add_css_class("pill");
        play_video_btn.set_visible(false);

        play_actions_box.append(&play_btn);
        play_actions_box.append(&play_video_btn);
        content_box.append(&play_actions_box);

        // Output Files & Actions Preferences Group (Responsive Libadwaita Rows)
        let files_group = libadwaita::PreferencesGroup::new();
        files_group.set_title("Generated Artifacts & Actions");
        files_group.set_margin_top(12);

        // Subtitles Row
        let subtitles_row = libadwaita::ActionRow::new();
        subtitles_row.set_title("Synchronized Subtitles");
        subtitles_row.set_subtitle("Industry standard .srt subtitle track");
        let open_subtitles_btn = gtk4::Button::with_label("Open .srt");
        open_subtitles_btn.add_css_class("flat");
        open_subtitles_btn.set_valign(gtk4::Align::Center);
        subtitles_row.add_suffix(&open_subtitles_btn);
        subtitles_row.set_visible(false);
        files_group.add(&subtitles_row);

        // Bilingual Script Row
        let script_row = libadwaita::ActionRow::new();
        script_row.set_title("Bilingual Transcript");
        script_row.set_subtitle("Side-by-side original and translated dialogue");
        let open_script_btn = gtk4::Button::with_label("View .txt");
        open_script_btn.add_css_class("flat");
        open_script_btn.set_valign(gtk4::Align::Center);
        script_row.add_suffix(&open_script_btn);
        script_row.set_visible(false);
        files_group.add(&script_row);

        // Containing Folder Row
        let folder_row = libadwaita::ActionRow::new();
        folder_row.set_title("Output Directory");
        folder_row.set_subtitle("Open the local directory containing all output files");
        let open_folder_btn = gtk4::Button::with_label("Open Folder");
        open_folder_btn.add_css_class("flat");
        open_folder_btn.set_valign(gtk4::Align::Center);
        folder_row.add_suffix(&open_folder_btn);
        files_group.add(&folder_row);

        content_box.append(&files_group);

        // Restart / New Translation Button
        let new_btn = gtk4::Button::with_label("Start Another Translation");
        new_btn.add_css_class("pill");
        new_btn.set_halign(gtk4::Align::Center);
        new_btn.set_margin_top(16);
        content_box.append(&new_btn);

        clamp.set_child(Some(&content_box));
        scroller.set_child(Some(&clamp));

        let current_output_path = Rc::new(RefCell::new(None::<PathBuf>));
        let current_video_path = Rc::new(RefCell::new(None::<PathBuf>));
        let current_subtitle_path = Rc::new(RefCell::new(None::<PathBuf>));
        let current_txt_path = Rc::new(RefCell::new(None::<PathBuf>));

        // Connect copy path
        let path_clone_copy = current_output_path.clone();
        copy_path_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_clone_copy.borrow() {
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&path.to_string_lossy());
                }
            }
        });

        // Connect open folder action
        let path_clone_folder = current_output_path.clone();
        open_folder_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_clone_folder.borrow() {
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

        // Connect play video action
        let vid_clone = current_video_path.clone();
        play_video_btn.connect_clicked(move |_| {
            if let Some(ref path) = *vid_clone.borrow() {
                let _ = std::process::Command::new("xdg-open").arg(path).spawn();
            }
        });

        // Connect open subtitles action
        let sub_clone = current_subtitle_path.clone();
        open_subtitles_btn.connect_clicked(move |_| {
            if let Some(ref path) = *sub_clone.borrow() {
                let _ = std::process::Command::new("xdg-open").arg(path).spawn();
            }
        });

        // Connect open bilingual script action
        let txt_clone = current_txt_path.clone();
        open_script_btn.connect_clicked(move |_| {
            if let Some(ref path) = *txt_clone.borrow() {
                let _ = std::process::Command::new("xdg-open").arg(path).spawn();
            }
        });

        Self {
            scroller,
            info_label,
            path_label,
            warning_box,
            warning_label,
            play_btn,
            play_video_btn,
            subtitles_row,
            script_row,
            new_btn,
            current_output_path,
            current_video_path,
            current_subtitle_path,
            current_txt_path,
        }
    }

    pub fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scroller
    }

    pub fn set_result(&self, artifact: &AudioArtifact, source_lang: &str, target_lang: &str) {
        *self.current_output_path.borrow_mut() = Some(artifact.path.clone());
        *self.current_video_path.borrow_mut() = artifact.video_path.clone();
        *self.current_subtitle_path.borrow_mut() = artifact.subtitle_srt_path.clone();
        *self.current_txt_path.borrow_mut() = artifact.transcript_txt_path.clone();

        self.play_video_btn
            .set_visible(artifact.video_path.is_some());
        self.subtitles_row
            .set_visible(artifact.subtitle_srt_path.is_some());
        self.script_row
            .set_visible(artifact.transcript_txt_path.is_some());

        let mins = artifact.duration_ms / 60000;
        let secs = (artifact.duration_ms % 60000) / 1000;
        let size_mb = (artifact.size_bytes as f64) / (1024.0 * 1024.0);

        let mut details = format!(
            "Translated from {} → {}\nDuration: {:02}:{:02} · Audio: {:.2} MB",
            source_lang, target_lang, mins, secs, size_mb
        );

        if let Some(ref vid) = artifact.video_path {
            details.push_str(&format!(
                "\n🎬 Dubbed Video: {}",
                vid.file_name().and_then(|f| f.to_str()).unwrap_or("video")
            ));
        }

        self.info_label.set_text(&details);
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
