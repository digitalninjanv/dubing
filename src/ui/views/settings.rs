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
        settings: AppSettings,
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

        let test_row = libadwaita::ActionRow::new();
        test_row.set_title("Test API Connection");
        test_row.set_subtitle("Verify API key access and quota status");

        let spinner = gtk4::Spinner::new();
        spinner.set_visible(false);

        let test_btn = gtk4::Button::with_label("Test Connection");
        test_btn.set_valign(gtk4::Align::Center);
        test_btn.add_css_class("flat");

        let test_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        test_box.set_valign(gtk4::Align::Center);
        test_box.append(&spinner);
        test_box.append(&test_btn);
        test_row.add_suffix(&test_box);

        let key_row_ref = key_row.clone();
        let test_row_clone = test_row.clone();
        let spinner_clone = spinner.clone();
        let btn_clone = test_btn.clone();

        test_btn.connect_clicked(move |_| {
            let key = key_row_ref.text().to_string();
            if key.trim().is_empty() {
                test_row_clone.set_subtitle("⚠ Please enter an API key first");
                return;
            }

            spinner_clone.set_visible(true);
            spinner_clone.start();
            btn_clone.set_sensitive(false);
            test_row_clone.set_subtitle("Testing connection to Google Gemini API...");

            let row_for_async = test_row_clone.clone();
            let spin_for_async = spinner_clone.clone();
            let btn_for_async = btn_clone.clone();

            glib::MainContext::default().spawn_local(async move {
                let client = crate::infrastructure::gemini::GeminiClient::new(&key);
                match client.test_connection().await {
                    Ok(models) => {
                        spin_for_async.stop();
                        spin_for_async.set_visible(false);
                        btn_for_async.set_sensitive(true);
                        let count = models.len();
                        row_for_async.set_subtitle(&format!(
                            "✓ Connected successfully! ({} Gemini models available)",
                            count
                        ));
                    }
                    Err(err) => {
                        spin_for_async.stop();
                        spin_for_async.set_visible(false);
                        btn_for_async.set_sensitive(true);
                        row_for_async.set_subtitle(&format!("✗ Connection failed: {}", err));
                    }
                }
            });
        });

        api_group.add(&key_row);
        api_group.add(&test_row);
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
        bitrate_row.set_selected(match settings.audio.default_bitrate_kbps {
            128 => 0,
            256 => 2,
            320 => 3,
            _ => 1,
        });
        audio_group.add(&bitrate_row);

        let cleanup_row = libadwaita::SwitchRow::new();
        cleanup_row.set_title("Auto Cleanup Temporary Audio");
        cleanup_row.set_subtitle("Delete intermediate segment wav files after export");
        cleanup_row.set_active(settings.auto_cleanup);
        audio_group.add(&cleanup_row);

        let debug_row = libadwaita::SwitchRow::new();
        debug_row.set_title("Debug Mode");
        debug_row.set_subtitle("Preserve intermediate segment audio and diagnostic logs");
        debug_row.set_active(settings.debug_mode);
        audio_group.add(&debug_row);

        // Persist audio preferences on change.
        let persist = std::rc::Rc::new({
            let bitrate_row = bitrate_row.clone();
            let cleanup_row = cleanup_row.clone();
            let debug_row = debug_row.clone();
            move || {
                let mut s = AppSettings::load();
                s.audio.default_bitrate_kbps = match bitrate_row.selected() {
                    0 => 128,
                    2 => 256,
                    3 => 320,
                    _ => 192,
                };
                s.auto_cleanup = cleanup_row.is_active();
                s.debug_mode = debug_row.is_active();
                if let Err(e) = s.save() {
                    tracing::warn!("Failed to persist settings: {}", e);
                }
            }
        });
        {
            let persist = persist.clone();
            bitrate_row.connect_selected_notify(move |_| persist());
        }
        {
            let persist = persist.clone();
            cleanup_row.connect_active_notify(move |_| persist());
        }
        {
            let persist = persist.clone();
            debug_row.connect_active_notify(move |_| persist());
        }

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
