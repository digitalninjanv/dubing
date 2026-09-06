use crate::domain::{TtsStylePreset, VoiceProfile};
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone)]
pub struct TtsStudioView {
    scroller: gtk4::ScrolledWindow,
    text_buffer: gtk4::TextBuffer,
    voice_combo: libadwaita::ComboRow,
    style_combo: libadwaita::ComboRow,
    custom_style_entry: libadwaita::EntryRow,
    speed_combo: libadwaita::ComboRow,
    generate_btn: gtk4::Button,
    spinner: gtk4::Spinner,
    status_label: gtk4::Label,
    result_group: libadwaita::PreferencesGroup,
    result_row: libadwaita::ActionRow,
    export_btn: gtk4::Button,
    current_audio_path: Rc<RefCell<Option<PathBuf>>>,
}

impl Default for TtsStudioView {
    fn default() -> Self {
        Self::new()
    }
}

impl TtsStudioView {
    pub fn new() -> Self {
        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scroller.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
        scroller.set_propagate_natural_width(false);
        scroller.set_propagate_natural_height(false);
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);

        let clamp = libadwaita::Clamp::new();
        clamp.set_maximum_size(780);
        clamp.set_tightening_threshold(580);
        clamp.set_vexpand(true);

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
        container.set_margin_top(24);
        container.set_margin_bottom(32);
        container.set_margin_start(20);
        container.set_margin_end(20);

        // Header Section
        let header_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        let title_label = gtk4::Label::new(Some("TTS Studio"));
        title_label.add_css_class("title-1");
        title_label.set_halign(gtk4::Align::Start);

        let subtitle_label = gtk4::Label::new(Some(
            "Synthesize expressive speech with customizable character, tone, and pacing like Google AI Studio.",
        ));
        subtitle_label.add_css_class("body");
        subtitle_label.add_css_class("dim-label");
        subtitle_label.set_wrap(true);
        subtitle_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        subtitle_label.set_halign(gtk4::Align::Start);

        header_box.append(&title_label);
        header_box.append(&subtitle_label);
        container.append(&header_box);

        // Group 1: Text Script Input
        let text_group = libadwaita::PreferencesGroup::new();
        text_group.set_title("Script to Synthesize");
        text_group.set_description(Some("Enter or paste text in any language"));

        let text_card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        text_card.add_css_class("card");
        text_card.set_margin_top(4);
        text_card.set_margin_bottom(4);

        let text_view = gtk4::TextView::new();
        text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        text_view.set_height_request(130);
        text_view.set_left_margin(12);
        text_view.set_right_margin(12);
        text_view.set_top_margin(12);
        text_view.set_bottom_margin(12);

        let text_buffer = text_view.buffer();
        text_buffer.set_text("Halo! Selamat datang di AudioDub AI TTS Studio. Di sini kamu bisa menghasilkan suara alami dengan gaya bicara yang fleksibel dan ekspresif.");

        let text_scroller = gtk4::ScrolledWindow::new();
        text_scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
        text_scroller.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
        text_scroller.set_min_content_height(120);
        text_scroller.set_child(Some(&text_view));
        text_card.append(&text_scroller);

        // Counter & Quick Sample Bar
        let bar_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        bar_box.set_margin_start(12);
        bar_box.set_margin_end(12);
        bar_box.set_margin_bottom(8);

        let char_count = text_buffer.char_count();
        let char_label = gtk4::Label::new(Some(&format!("{} chars", char_count)));
        char_label.add_css_class("caption");
        char_label.add_css_class("dim-label");
        char_label.set_hexpand(true);
        char_label.set_halign(gtk4::Align::Start);

        let label_ref = char_label.clone();
        text_buffer.connect_changed(move |buf| {
            label_ref.set_text(&format!("{} chars", buf.char_count()));
        });

        let sample_id_btn = gtk4::Button::with_label("Sample ID");
        sample_id_btn.add_css_class("flat");
        sample_id_btn.add_css_class("caption");
        let buf_id = text_buffer.clone();
        sample_id_btn.connect_clicked(move |_| {
            buf_id.set_text("Teknologi kecerdasan buatan kini mampu menghasilkan suara yang terdengar sangat manusiawi dan penuh penghayatan emosi.");
        });

        let sample_en_btn = gtk4::Button::with_label("Sample EN");
        sample_en_btn.add_css_class("flat");
        sample_en_btn.add_css_class("caption");
        let buf_en = text_buffer.clone();
        sample_en_btn.connect_clicked(move |_| {
            buf_en.set_text("Welcome to AudioDub AI. With Gemini speech models, you can adjust vocal tone, dynamic pacing, and dramatic nuance seamlessly.");
        });

        let clear_btn = gtk4::Button::with_label("Clear");
        clear_btn.add_css_class("flat");
        clear_btn.add_css_class("caption");
        let buf_clear = text_buffer.clone();
        clear_btn.connect_clicked(move |_| {
            buf_clear.set_text("");
        });

        bar_box.append(&char_label);
        bar_box.append(&sample_id_btn);
        bar_box.append(&sample_en_btn);
        bar_box.append(&clear_btn);
        text_card.append(&bar_box);

        text_group.add(&text_card);
        container.append(&text_group);

        // Group 2: Voice & Style Settings
        let style_group = libadwaita::PreferencesGroup::new();
        style_group.set_title("Voice & Speech Style");
        style_group.set_description(Some("Configure voice profile, emotion, and cadence"));

        // Voice Picker
        let voice_options = [
            "Puck — Youthful, bright & animated",
            "Charon — Deep, authoritative & mature",
            "Kore — Firm, professional & clear",
            "Fenrir — Resonant, cinematic & dramatic",
            "Aoede — Warm, breezy & melodic",
        ];
        let voice_model = gtk4::StringList::new(&voice_options);
        let voice_combo = libadwaita::ComboRow::new();
        voice_combo.set_title("Voice Character");
        voice_combo.set_subtitle("Neural voice model timbre");
        voice_combo.set_model(Some(&voice_model));
        voice_combo.set_selected(0);
        style_group.add(&voice_combo);

        // Style Preset
        let style_options = [
            "Natural & Conversational (Default relaxed pacing)",
            "Storyteller (Dramatic, expressive pauses & dynamic inflection)",
            "News Broadcaster (Formal, clear enunciation & steady tone)",
            "Energetic & Cheerful (High energy, upbeat podcast style)",
            "Calm & Meditative (Soft, soothing, ASMR pacing)",
            "Custom Directive (Freeform prompt instructions)",
        ];
        let style_model = gtk4::StringList::new(&style_options);
        let style_combo = libadwaita::ComboRow::new();
        style_combo.set_title("Speaking Style Preset");
        style_combo.set_subtitle("Emotional tone and delivery style");
        style_combo.set_model(Some(&style_model));
        style_combo.set_selected(0);
        style_group.add(&style_combo);

        // Custom Style EntryRow (Google AI Studio prompt style)
        let custom_style_entry = libadwaita::EntryRow::new();
        custom_style_entry.set_title("Custom Style Directive");
        custom_style_entry.set_show_apply_button(false);
        style_group.add(&custom_style_entry);

        // Update custom entry hint based on preset
        let entry_ref = custom_style_entry.clone();
        style_combo.connect_selected_notify(move |combo| {
            match combo.selected() {
                0 => entry_ref.set_text(""),
                1 => entry_ref.set_text("Speak like an expressive and captivating storyteller, with dramatic pauses and emotional inflection."),
                2 => entry_ref.set_text("Speak in a professional, authoritative, and articulate news broadcaster style with steady cadence."),
                3 => entry_ref.set_text("Speak with high energy, enthusiasm, and an upbeat, friendly tone."),
                4 => entry_ref.set_text("Speak in a calm, gentle, and soothing tone with slow, relaxing pacing."),
                5 if entry_ref.text().is_empty() => {
                    entry_ref.set_text("Speak softly with a warm tone and thoughtful pauses...");
                }
                _ => {}
            }
        });

        // Speed Selector
        let speed_options = [
            "0.80x — Relaxed & measured",
            "1.00x — Normal speaking tempo",
            "1.15x — Brisk & upbeat",
            "1.30x — Fast delivery",
        ];
        let speed_model = gtk4::StringList::new(&speed_options);
        let speed_combo = libadwaita::ComboRow::new();
        speed_combo.set_title("Speaking Speed");
        speed_combo.set_subtitle("Tempo and pacing multiplier");
        speed_combo.set_model(Some(&speed_model));
        speed_combo.set_selected(1); // Default 1.0x
        style_group.add(&speed_combo);

        container.append(&style_group);

        // Action Section
        let action_box = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        action_box.set_margin_top(8);

        let generate_btn = gtk4::Button::with_label("Generate Speech");
        generate_btn.set_icon_name("media-playback-start-symbolic");
        generate_btn.add_css_class("suggested-action");
        generate_btn.add_css_class("pill");
        generate_btn.set_height_request(46);
        generate_btn.set_halign(gtk4::Align::Center);
        generate_btn.set_width_request(240);

        let spinner = gtk4::Spinner::new();
        spinner.set_size_request(28, 28);
        spinner.set_halign(gtk4::Align::Center);
        spinner.set_visible(false);

        let status_label = gtk4::Label::new(None);
        status_label.add_css_class("caption");
        status_label.add_css_class("dim-label");
        status_label.set_wrap(true);
        status_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        status_label.set_halign(gtk4::Align::Center);

        action_box.append(&generate_btn);
        action_box.append(&spinner);
        action_box.append(&status_label);
        container.append(&action_box);

        // Group 3: Result & Playback Card
        let result_group = libadwaita::PreferencesGroup::new();
        result_group.set_title("Generated Speech");
        result_group.set_description(Some("Audio output ready for preview and export"));
        result_group.set_visible(false); // Hidden until audio generated

        let result_row = libadwaita::ActionRow::new();
        result_row.set_title("Audio Preview");
        result_row.set_subtitle("No audio generated yet");

        let play_btn = gtk4::Button::from_icon_name("media-playback-start-symbolic");
        play_btn.set_tooltip_text(Some("Play Audio Preview"));
        play_btn.add_css_class("flat");
        play_btn.add_css_class("circular");
        play_btn.set_valign(gtk4::Align::Center);
        result_row.add_prefix(&play_btn);

        let copy_path_btn = gtk4::Button::from_icon_name("edit-copy-symbolic");
        copy_path_btn.set_tooltip_text(Some("Copy File Path"));
        copy_path_btn.add_css_class("flat");
        copy_path_btn.set_valign(gtk4::Align::Center);
        result_row.add_suffix(&copy_path_btn);

        let open_folder_btn = gtk4::Button::from_icon_name("folder-open-symbolic");
        open_folder_btn.set_tooltip_text(Some("Open Containing Folder"));
        open_folder_btn.add_css_class("flat");
        open_folder_btn.set_valign(gtk4::Align::Center);
        result_row.add_suffix(&open_folder_btn);

        let export_btn = gtk4::Button::with_label("Export MP3...");
        export_btn.set_icon_name("document-save-symbolic");
        export_btn.add_css_class("suggested-action");
        export_btn.set_valign(gtk4::Align::Center);
        result_row.add_suffix(&export_btn);

        result_group.add(&result_row);
        container.append(&result_group);

        clamp.set_child(Some(&container));
        scroller.set_child(Some(&clamp));

        let current_audio_path = Rc::new(RefCell::new(None::<PathBuf>));

        // Connect copy path
        let path_copy = current_audio_path.clone();
        copy_path_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_copy.borrow() {
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&path.to_string_lossy());
                }
            }
        });

        // Connect open folder
        let path_folder = current_audio_path.clone();
        open_folder_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_folder.borrow() {
                if let Some(parent) = path.parent() {
                    let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                }
            }
        });

        // Connect play button
        let path_play = current_audio_path.clone();
        play_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_play.borrow() {
                let _ = std::process::Command::new("xdg-open").arg(path).spawn();
            }
        });

        Self {
            scroller,
            text_buffer,
            voice_combo,
            style_combo,
            custom_style_entry,
            speed_combo,
            generate_btn,
            spinner,
            status_label,
            result_group,
            result_row,
            export_btn,
            current_audio_path,
        }
    }

    pub fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scroller
    }

    pub fn text(&self) -> String {
        let start = self.text_buffer.start_iter();
        let end = self.text_buffer.end_iter();
        self.text_buffer.text(&start, &end, false).to_string()
    }

    pub fn selected_voice_name(&self) -> String {
        match self.voice_combo.selected() {
            0 => "Puck".to_string(),
            1 => "Charon".to_string(),
            2 => "Kore".to_string(),
            3 => "Fenrir".to_string(),
            4 => "Aoede".to_string(),
            _ => "Puck".to_string(),
        }
    }

    pub fn selected_style_preset(&self) -> TtsStylePreset {
        let custom_text = self.custom_style_entry.text().to_string();
        TtsStylePreset::from_index(self.style_combo.selected(), Some(custom_text))
    }

    pub fn selected_style_instruction(&self) -> Option<String> {
        let custom_text = self.custom_style_entry.text().to_string();
        if !custom_text.trim().is_empty() {
            Some(custom_text.trim().to_string())
        } else {
            let preset = self.selected_style_preset();
            let directive = preset.prompt_directive(None);
            Some(directive)
        }
    }

    pub fn selected_speed(&self) -> f32 {
        match self.speed_combo.selected() {
            0 => 0.8,
            1 => 1.0,
            2 => 1.15,
            3 => 1.3,
            _ => 1.0,
        }
    }

    pub fn build_voice_profile(&self) -> VoiceProfile {
        let voice_name = self.selected_voice_name();
        let speed = self.selected_speed();
        let style = self.selected_style_instruction();
        VoiceProfile {
            id: voice_name.to_lowercase(),
            voice_name,
            language: "auto".to_string(),
            style,
            speed,
        }
    }

    pub fn set_generating(&self, generating: bool, status: &str) {
        self.generate_btn.set_sensitive(!generating);
        self.spinner.set_visible(generating);
        if generating {
            self.spinner.start();
        } else {
            self.spinner.stop();
        }
        self.status_label.set_text(status);
    }

    pub fn set_result(&self, path: PathBuf, duration_ms: u64) {
        *self.current_audio_path.borrow_mut() = Some(path.clone());
        self.result_row.set_title(&format!(
            "Audio Preview ({:.1}s)",
            duration_ms as f64 / 1000.0
        ));
        self.result_row.set_subtitle(&path.to_string_lossy());
        self.result_group.set_visible(true);
    }

    pub fn current_audio_path(&self) -> Option<PathBuf> {
        self.current_audio_path.borrow().clone()
    }

    pub fn connect_generate_clicked<F: Fn() + 'static>(&self, callback: F) {
        self.generate_btn.connect_clicked(move |_| callback());
    }

    pub fn connect_export_clicked<F: Fn(PathBuf) + 'static>(&self, callback: F) {
        let path_clone = self.current_audio_path.clone();
        self.export_btn.connect_clicked(move |_| {
            if let Some(ref path) = *path_clone.borrow() {
                callback(path.clone());
            }
        });
    }
}
