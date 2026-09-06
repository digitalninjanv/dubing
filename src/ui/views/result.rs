use crate::domain::AudioArtifact;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone)]
pub struct ResultView {
    container: gtk4::Box,
    info_label: gtk4::Label,
    path_label: gtk4::Label,
    warning_box: gtk4::Box,
    warning_label: gtk4::Label,
    #[allow(dead_code)]
    play_btn: gtk4::Button,
    play_video_btn: gtk4::Button,
    open_subtitles_btn: gtk4::Button,
    open_bilingual_btn: gtk4::Button,
    #[allow(dead_code)]
    open_folder_btn: gtk4::Button,
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
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_margin_top(32);
        container.set_margin_bottom(32);
        container.set_margin_start(32);
        container.set_margin_end(32);

        let icon = gtk4::Image::from_icon_name("emblem-ok-symbolic");
        icon.set_pixel_size(64);
        icon.add_css_class("success");
        container.append(&icon);

        let title = gtk4::Label::new(Some("Dubbing & Translation Complete!"));
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

        // Action Buttons Row 1 (Media Playback & Subtitles)
        let media_actions_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        media_actions_box.set_halign(gtk4::Align::Center);
        media_actions_box.set_margin_top(20);

        let play_btn = gtk4::Button::with_label("▶ Play Dubbed Audio");
        play_btn.add_css_class("suggested-action");
        play_btn.add_css_class("pill");

        let play_video_btn = gtk4::Button::with_label("🎬 Play Dubbed Video");
        play_video_btn.add_css_class("suggested-action");
        play_video_btn.add_css_class("pill");
        play_video_btn.set_visible(false);

        let open_subtitles_btn = gtk4::Button::with_label("📄 Open Subtitles (.srt)");
        open_subtitles_btn.add_css_class("pill");
        open_subtitles_btn.set_visible(false);

        let open_bilingual_btn = gtk4::Button::with_label("📝 Bilingual Script (.txt)");
        open_bilingual_btn.add_css_class("pill");
        open_bilingual_btn.set_visible(false);

        media_actions_box.append(&play_btn);
        media_actions_box.append(&play_video_btn);
        media_actions_box.append(&open_subtitles_btn);
        media_actions_box.append(&open_bilingual_btn);
        container.append(&media_actions_box);

        // Action Buttons Row 2 (Folder & Reset)
        let actions_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        actions_box.set_halign(gtk4::Align::Center);
        actions_box.set_margin_top(12);

        let open_folder_btn = gtk4::Button::with_label("Open Containing Folder");
        open_folder_btn.add_css_class("pill");

        let new_btn = gtk4::Button::with_label("Start Another Translation");
        new_btn.add_css_class("pill");

        actions_box.append(&open_folder_btn);
        actions_box.append(&new_btn);
        container.append(&actions_box);

        let current_output_path = Rc::new(RefCell::new(None::<PathBuf>));
        let current_video_path = Rc::new(RefCell::new(None::<PathBuf>));
        let current_subtitle_path = Rc::new(RefCell::new(None::<PathBuf>));
        let current_txt_path = Rc::new(RefCell::new(None::<PathBuf>));

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

        // Connect open bilingual txt action
        let txt_clone = current_txt_path.clone();
        open_bilingual_btn.connect_clicked(move |_| {
            if let Some(ref path) = *txt_clone.borrow() {
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
            play_video_btn,
            open_subtitles_btn,
            open_bilingual_btn,
            open_folder_btn,
            new_btn,
            current_output_path,
            current_video_path,
            current_subtitle_path,
            current_txt_path,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    pub fn set_result(&self, artifact: &AudioArtifact, source_lang: &str, target_lang: &str) {
        *self.current_output_path.borrow_mut() = Some(artifact.path.clone());
        *self.current_video_path.borrow_mut() = artifact.video_path.clone();
        *self.current_subtitle_path.borrow_mut() = artifact.subtitle_srt_path.clone();
        *self.current_txt_path.borrow_mut() = artifact.transcript_txt_path.clone();

        self.play_video_btn
            .set_visible(artifact.video_path.is_some());
        self.open_subtitles_btn
            .set_visible(artifact.subtitle_srt_path.is_some());
        self.open_bilingual_btn
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
        if let Some(ref srt) = artifact.subtitle_srt_path {
            details.push_str(&format!(
                "\n📄 Subtitles: {}",
                srt.file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("subtitles.srt")
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
