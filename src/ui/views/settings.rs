use crate::application::ports::SecretStore;
use crate::config::AppSettings;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::sync::Arc;

pub struct SettingsDialog;

impl SettingsDialog {
    pub fn show(
        parent: &impl IsA<gtk4::Window>,
        secret_store: Arc<dyn SecretStore>,
        _settings: AppSettings,
    ) {
        let window = libadwaita::PreferencesWindow::new();
        window.set_transient_for(Some(parent));
        window.set_title(Some("Settings — AudioDub AI"));
        window.set_modal(true);
        window.set_default_size(600, 480);

        let page = libadwaita::PreferencesPage::new();
        page.set_title("General");
        page.set_icon_name(Some("preferences-system-symbolic"));

        // 1. AI API Key Group
        let api_group = libadwaita::PreferencesGroup::new();
        api_group.set_title("Google Gemini API");
        api_group.set_description(Some(
            "An API key from Google AI Studio (ai.google.dev) is required for speech and translation services.",
        ));

        let key_row = libadwaita::PasswordEntryRow::new();
        key_row.set_title("API Key");

        if let Ok(Some(existing_key)) = secret_store.get_api_key() {
            key_row.set_text(&existing_key);
        }

        let store_clone = secret_store.clone();
        key_row.connect_changed(move |entry| {
            let text = entry.text();
            if !text.is_empty() {
                let _ = store_clone.set_api_key(&text);
            }
        });

        api_group.add(&key_row);
        page.add(&api_group);

        // 2. Audio Preferences Group
        let audio_group = libadwaita::PreferencesGroup::new();
        audio_group.set_title("Audio Output");

        let bitrate_row = libadwaita::ComboRow::new();
        bitrate_row.set_title("MP3 Bitrate");
        bitrate_row.set_subtitle("Default audio encoding bitrate");
        let bitrates =
            gtk4::StringList::new(&["128 kbps", "192 kbps (Recommended)", "256 kbps", "320 kbps"]);
        bitrate_row.set_model(Some(&bitrates));
        bitrate_row.set_selected(1);
        audio_group.add(&bitrate_row);

        let cleanup_row = libadwaita::SwitchRow::new();
        cleanup_row.set_title("Auto Cleanup Temporary Audio");
        cleanup_row.set_subtitle("Delete intermediate segment wav files after export");
        cleanup_row.set_active(_settings.auto_cleanup);
        audio_group.add(&cleanup_row);

        let debug_row = libadwaita::SwitchRow::new();
        debug_row.set_title("Debug Mode");
        debug_row.set_subtitle("Preserve intermediate segment audio and diagnostic logs");
        debug_row.set_active(_settings.debug_mode);
        audio_group.add(&debug_row);

        page.add(&audio_group);

        // 3. Privacy & Compliance Group (PRD SEC-005)
        let privacy_group = libadwaita::PreferencesGroup::new();
        privacy_group.set_title("Privacy & Data Usage");
        privacy_group.set_description(Some(
            "Audio submitted for transcription, translation, and voice synthesis is processed via Google Gemini APIs. \
            Ensure you have the necessary rights or authorization for the audio materials you upload. AudioDub AI does not \
            store your audio on external servers beyond temporary API processing windows (up to 48 hours for Gemini File API)."
        ));
        page.add(&privacy_group);

        window.add(&page);
        window.present();
    }
}
