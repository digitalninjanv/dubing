use crate::domain::{LanguageId, LanguageRegistry, SpeakerVoiceConfig, TranslationTone};
use crate::ui::components::LanguagePickerHelper;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone)]
pub struct DropzoneView {
    container: gtk4::Box,
    choose_file_btn: gtk4::Button,
    selected_file_label: gtk4::Label,
    file_info_box: gtk4::Box,
    translate_btn: gtk4::Button,
    source_combo: libadwaita::ComboRow,
    target_combo: libadwaita::ComboRow,
    tone_combo: libadwaita::ComboRow,
    speaker1_combo: libadwaita::ComboRow,
    speaker2_combo: libadwaita::ComboRow,
    source_ids: Vec<LanguageId>,
    target_ids: Vec<LanguageId>,
    selected_path: Rc<RefCell<Option<PathBuf>>>,
}

impl DropzoneView {
    pub fn new(registry: &LanguageRegistry) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_margin_top(24);
        container.set_margin_bottom(24);
        container.set_margin_start(24);
        container.set_margin_end(24);

        // Header / Welcome banner
        let title_label = gtk4::Label::new(Some("Translate Your Audio"));
        title_label.add_css_class("title-1");

        let subtitle_label = gtk4::Label::new(Some(
            "Drop your audio file here or choose a file to transcribe, translate, and synthesize.",
        ));
        subtitle_label.add_css_class("dim-label");
        subtitle_label.set_wrap(true);

        container.append(&title_label);
        container.append(&subtitle_label);

        // Dropzone Card / Frame
        let drop_frame = gtk4::Frame::new(None);
        drop_frame.add_css_class("card");
        drop_frame.set_valign(gtk4::Align::Center);

        let drop_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        drop_box.set_margin_top(32);
        drop_box.set_margin_bottom(32);
        drop_box.set_margin_start(32);
        drop_box.set_margin_end(32);

        let drop_icon = gtk4::Image::from_icon_name("audio-x-generic-symbolic");
        drop_icon.set_pixel_size(64);
        drop_icon.add_css_class("dim-label");

        let drop_prompt = gtk4::Label::new(Some("Drop audio file here"));
        drop_prompt.add_css_class("title-3");

        let or_label = gtk4::Label::new(Some("or"));
        or_label.add_css_class("dim-label");

        let choose_file_btn = gtk4::Button::with_label("Choose a File…");
        choose_file_btn.add_css_class("pill");
        choose_file_btn.set_halign(gtk4::Align::Center);

        drop_box.append(&drop_icon);
        drop_box.append(&drop_prompt);
        drop_box.append(&or_label);
        drop_box.append(&choose_file_btn);
        drop_frame.set_child(Some(&drop_box));
        container.append(&drop_frame);

        // Selected File Status Box
        let file_info_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        file_info_box.add_css_class("card");
        file_info_box.set_margin_top(8);
        file_info_box.set_margin_bottom(8);
        file_info_box.set_margin_start(8);
        file_info_box.set_margin_end(8);
        file_info_box.set_visible(false);

        let file_icon = gtk4::Image::from_icon_name("audio-x-generic-symbolic");
        file_icon.set_pixel_size(24);
        let selected_file_label = gtk4::Label::new(None);
        selected_file_label.set_hexpand(true);
        selected_file_label.set_halign(gtk4::Align::Start);
        selected_file_label.add_css_class("heading");

        file_info_box.append(&file_icon);
        file_info_box.append(&selected_file_label);
        container.append(&file_info_box);

        // Language Selection Group
        let pref_group = libadwaita::PreferencesGroup::new();
        pref_group.set_title("Translation Settings");

        let (source_model, source_ids) = LanguagePickerHelper::create_source_model(registry);
        let source_combo = libadwaita::ComboRow::new();
        source_combo.set_title("Source Language");
        source_combo.set_subtitle("Audio spoken language");
        source_combo.set_model(Some(&source_model));
        source_combo.set_selected(0); // Default to Auto Detect

        let (target_model, target_ids) = LanguagePickerHelper::create_target_model(registry);
        let target_combo = libadwaita::ComboRow::new();
        target_combo.set_title("Target Language");
        target_combo.set_subtitle("Voice language to generate");
        target_combo.set_model(Some(&target_model));
        target_combo.set_selected(0);

        pref_group.add(&source_combo);
        pref_group.add(&target_combo);

        // Translation Tone Selector
        let tone_items = [
            "Neutral (Standard, natural speech)",
            "Casual (Conversational & relaxed)",
            "Formal (Professional & polite)",
            "Creative (Expressive & dramatic)",
        ];
        let tone_model = gtk4::StringList::new(&tone_items);
        let tone_combo = libadwaita::ComboRow::new();
        tone_combo.set_title("Translation Tone");
        tone_combo.set_subtitle("Adjust style, formality, and phrasing");
        tone_combo.set_model(Some(&tone_model));
        tone_combo.set_selected(0);
        pref_group.add(&tone_combo);

        // Voice Profile Expander
        let voice_options = [
            "Auto (Recommended)",
            "Kore (Firm & professional)",
            "Puck (Upbeat & clear)",
            "Fenrir (Excited & resonant)",
            "Aoede (Breezy & soft)",
        ];
        let voice_expander = libadwaita::ExpanderRow::new();
        voice_expander.set_title("Speaker Voices (Optional)");
        voice_expander.set_subtitle("Assign specific voices to speakers");

        let s1_model = gtk4::StringList::new(&voice_options);
        let speaker1_combo = libadwaita::ComboRow::new();
        speaker1_combo.set_title("Speaker 1 Voice");
        speaker1_combo.set_model(Some(&s1_model));
        speaker1_combo.set_selected(0);

        let s2_model = gtk4::StringList::new(&voice_options);
        let speaker2_combo = libadwaita::ComboRow::new();
        speaker2_combo.set_title("Speaker 2 Voice");
        speaker2_combo.set_model(Some(&s2_model));
        speaker2_combo.set_selected(0);

        voice_expander.add_row(&speaker1_combo);
        voice_expander.add_row(&speaker2_combo);
        pref_group.add(&voice_expander);

        container.append(&pref_group);

        // Translate & Dub Action Button
        let translate_btn = gtk4::Button::with_label("Translate & Dub Media");
        translate_btn.add_css_class("suggested-action");
        translate_btn.add_css_class("pill");
        translate_btn.set_halign(gtk4::Align::Center);
        translate_btn.set_sensitive(false); // Inactive until valid file is selected
        translate_btn.set_margin_top(16);
        container.append(&translate_btn);

        let selected_path = Rc::new(RefCell::new(None));

        Self {
            container,
            choose_file_btn,
            selected_file_label,
            file_info_box,
            translate_btn,
            source_combo,
            target_combo,
            tone_combo,
            speaker1_combo,
            speaker2_combo,
            source_ids,
            target_ids,
            selected_path,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    pub fn set_selected_file(&self, path: PathBuf) {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio")
            .to_string();

        self.selected_file_label.set_text(&name);
        self.file_info_box.set_visible(true);
        self.translate_btn.set_sensitive(true);
        *self.selected_path.borrow_mut() = Some(path);
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_path.borrow().clone()
    }

    pub fn selected_source_language(&self) -> LanguageId {
        let idx = self.source_combo.selected() as usize;
        self.source_ids
            .get(idx)
            .cloned()
            .unwrap_or_else(LanguageId::auto)
    }

    pub fn selected_target_language(&self) -> LanguageId {
        let idx = self.target_combo.selected() as usize;
        self.target_ids
            .get(idx)
            .cloned()
            .unwrap_or_else(|| LanguageId::new("en"))
    }

    pub fn selected_tone(&self) -> TranslationTone {
        match self.tone_combo.selected() {
            1 => TranslationTone::Casual,
            2 => TranslationTone::Formal,
            3 => TranslationTone::Creative,
            _ => TranslationTone::Neutral,
        }
    }

    pub fn selected_voice_config(&self) -> Option<SpeakerVoiceConfig> {
        let extract_voice = |idx: u32| -> Option<String> {
            match idx {
                1 => Some("Kore".to_string()),
                2 => Some("Puck".to_string()),
                3 => Some("Fenrir".to_string()),
                4 => Some("Aoede".to_string()),
                _ => None,
            }
        };

        let s1 = extract_voice(self.speaker1_combo.selected());
        let s2 = extract_voice(self.speaker2_combo.selected());

        if s1.is_some() || s2.is_some() {
            Some(SpeakerVoiceConfig::new(s1, s2))
        } else {
            None
        }
    }

    pub fn connect_choose_file<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.choose_file_btn.connect_clicked(move |_| callback());
    }

    pub fn connect_translate_clicked<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.translate_btn.connect_clicked(move |_| callback());
    }
}
