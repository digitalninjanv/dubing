use crate::application::live_dubber::LiveSessionOptions;
use crate::application::ports::AudioRouter;
use crate::application::ports::SecretStore;
use crate::application::LiveDubberOrchestrator;
use crate::config::AppSettings;
use crate::domain::{
    AudioAppInfo, AudioSourceMode, LanguageId, LanguageRegistry, LiveDubberStatus, LiveModelChoice,
    LiveTranscriptUpdate,
};
use crate::infrastructure::audio_router::PactlAudioRouter;
use crate::infrastructure::gemini::{GeminiClient, GeminiLiveStreamer};
use crate::ui::views::SettingsDialog;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Clone)]
pub struct LiveDubberView {
    scroller: gtk4::ScrolledWindow,
    status_pill: gtk4::Label,
    status_desc: gtk4::Label,
    source_combo: libadwaita::ComboRow,
    apps_combo: libadwaita::ComboRow,
    refresh_apps_btn: gtk4::Button,
    model_combo: libadwaita::ComboRow,
    lang_combo: libadwaita::ComboRow,
    voice_combo: libadwaita::ComboRow,
    start_stop_btn: gtk4::Button,
    spinner: gtk4::Spinner,
    original_buffer: gtk4::TextBuffer,
    translated_buffer: gtk4::TextBuffer,
    detected_apps: Rc<RefCell<Vec<AudioAppInfo>>>,
    is_running: Rc<RefCell<bool>>,
    cancel_token: Rc<RefCell<Option<CancellationToken>>>,
}

impl LiveDubberView {
    pub fn new(registry: &Arc<LanguageRegistry>) -> Self {
        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scroller.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
        scroller.set_propagate_natural_width(false);
        scroller.set_propagate_natural_height(false);
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);

        let clamp = libadwaita::Clamp::new();
        clamp.set_maximum_size(820);
        clamp.set_tightening_threshold(600);
        clamp.set_vexpand(true);

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
        container.set_margin_top(24);
        container.set_margin_bottom(32);
        container.set_margin_start(20);
        container.set_margin_end(20);

        // 1. Header Section
        let header_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        let title_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);

        let title_label = gtk4::Label::new(Some("Live Stream Dubber"));
        title_label.add_css_class("title-1");
        title_label.set_halign(gtk4::Align::Start);

        let status_pill = gtk4::Label::new(Some("Idle"));
        status_pill.add_css_class("caption");
        status_pill.add_css_class("accent");
        status_pill.set_valign(gtk4::Align::Center);
        status_pill.set_margin_start(8);

        title_box.append(&title_label);
        title_box.append(&status_pill);

        let subtitle_label = gtk4::Label::new(Some(
            "Direct real-time speech interpretation for YouTube & desktop audio. Mutes original sound and plays dubbed voice instantly.",
        ));
        subtitle_label.add_css_class("dim-label");
        subtitle_label.set_halign(gtk4::Align::Start);
        subtitle_label.set_wrap(true);

        let status_desc = gtk4::Label::new(Some("Ready to connect to Gemini Multimodal Live API"));
        status_desc.add_css_class("body");
        status_desc.set_halign(gtk4::Align::Start);

        header_box.append(&title_box);
        header_box.append(&subtitle_label);
        header_box.append(&status_desc);
        container.append(&header_box);

        // 2. Audio Capture Configuration Group
        let audio_group = libadwaita::PreferencesGroup::new();
        audio_group.set_title("Audio Capture & Routing");
        audio_group.set_description(Some(
            "Route YouTube / Browser audio into AudioDub's virtual null-sink to silence the original language.",
        ));

        let source_model = gtk4::StringList::new(&[
            "Browser / YouTube (Silences Original Audio via Virtual Sink)",
            "System Desktop Monitor (Captures All Sound)",
            "Microphone (Live Spoken Speech)",
        ]);
        let source_combo = libadwaita::ComboRow::new();
        source_combo.set_title("Audio Source");
        source_combo.set_subtitle("Select how live audio should be captured");
        source_combo.set_model(Some(&source_model));
        source_combo.set_selected(0);
        audio_group.add(&source_combo);

        let apps_model = gtk4::StringList::new(&["All Browser Windows (Auto Detect)"]);
        let apps_combo = libadwaita::ComboRow::new();
        apps_combo.set_title("Target Application");
        apps_combo.set_subtitle("Specific app to silence and dub (e.g. Chrome, Firefox)");
        apps_combo.set_model(Some(&apps_model));
        apps_combo.set_selected(0);

        let refresh_apps_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");
        refresh_apps_btn.set_valign(gtk4::Align::Center);
        refresh_apps_btn.set_tooltip_text(Some("Refresh running audio applications"));
        apps_combo.add_suffix(&refresh_apps_btn);
        audio_group.add(&apps_combo);

        container.append(&audio_group);

        // 3. AI Translation & Voice Configuration Group
        let ai_group = libadwaita::PreferencesGroup::new();
        ai_group.set_title("Live AI Interpretation Model");
        ai_group.set_description(Some(
            "Official Gemini Multimodal Live API WebSocket models with low-latency speech synthesis.",
        ));

        let model_list = gtk4::StringList::new(&[
            "Gemini 3.5 Live Translate Preview (Recommended Primary S2S)",
            "Gemini 3.1 Flash Live Preview (Conversational Fallback)",
            "Gemini 2.5 Flash Native Audio Dialog",
            "Gemini 3.5 Transcribe Live (Real-Time Subtitles Only)",
        ]);
        let model_combo = libadwaita::ComboRow::new();
        model_combo.set_title("Model Engine");
        model_combo.set_subtitle("Speech-to-speech translation or transcription");
        model_combo.set_model(Some(&model_list));
        model_combo.set_selected(0);
        ai_group.add(&model_combo);

        // Target Language Combo
        let target_langs = registry.list();
        let lang_titles: Vec<String> = target_langs
            .iter()
            .map(|l| format!("{} ({})", l.display_name, l.id.as_str()))
            .collect();
        let lang_strs: Vec<&str> = lang_titles.iter().map(|s| s.as_str()).collect();
        let lang_model = gtk4::StringList::new(&lang_strs);
        let lang_combo = libadwaita::ComboRow::new();
        lang_combo.set_title("Target Dubbing Language");
        lang_combo.set_subtitle("Language to speak in real-time");
        lang_combo.set_model(Some(&lang_model));

        // Default to Indonesian if found, otherwise first
        let default_idx = target_langs
            .iter()
            .position(|l| l.id.as_str() == "id")
            .unwrap_or(0);
        lang_combo.set_selected(default_idx as u32);
        ai_group.add(&lang_combo);

        // Voice Combo
        let voices = [
            "Aoede (Warm)",
            "Puck (Energetic)",
            "Charon (Deep)",
            "Kore (Calm)",
            "Fenrir (Expressive)",
        ];
        let voice_model = gtk4::StringList::new(&voices);
        let voice_combo = libadwaita::ComboRow::new();
        voice_combo.set_title("Dubbing Voice Persona");
        voice_combo.set_subtitle("Google Prebuilt Voice for natural speech");
        voice_combo.set_model(Some(&voice_model));
        voice_combo.set_selected(0);
        ai_group.add(&voice_combo);

        container.append(&ai_group);

        // 4. Action Button Area (Start / Stop)
        let action_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
        action_box.set_halign(gtk4::Align::Center);
        action_box.set_margin_top(8);
        action_box.set_margin_bottom(8);

        let start_stop_btn = gtk4::Button::with_label("Start Live Dubbing");
        start_stop_btn.add_css_class("suggested-action");
        start_stop_btn.add_css_class("pill");
        start_stop_btn.set_size_request(240, 50);

        let spinner = gtk4::Spinner::new();
        spinner.set_visible(false);

        action_box.append(&start_stop_btn);
        action_box.append(&spinner);
        container.append(&action_box);

        // 5. Live Transcripts & Subtitles Display
        let transcript_group = libadwaita::PreferencesGroup::new();
        transcript_group.set_title("Live Subtitles & Closed Captions");
        transcript_group.set_description(Some(
            "Simultaneous transcripts detected from the stream and translated in real-time.",
        ));

        let transcript_grid = gtk4::Grid::new();
        transcript_grid.set_column_spacing(12);
        transcript_grid.set_row_spacing(6);
        transcript_grid.set_column_homogeneous(true);

        // Left: Original Audio Text
        let orig_label = gtk4::Label::new(Some("Original Audio (Detected)"));
        orig_label.add_css_class("heading");
        orig_label.set_halign(gtk4::Align::Start);

        let orig_scroller = gtk4::ScrolledWindow::new();
        orig_scroller.set_min_content_height(140);
        orig_scroller.set_propagate_natural_height(false);
        orig_scroller.add_css_class("card");

        let original_buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
        let orig_text_view = gtk4::TextView::with_buffer(&original_buffer);
        orig_text_view.set_editable(false);
        orig_text_view.set_cursor_visible(false);
        orig_text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        orig_text_view.set_margin_top(8);
        orig_text_view.set_margin_bottom(8);
        orig_text_view.set_margin_start(8);
        orig_text_view.set_margin_end(8);
        orig_scroller.set_child(Some(&orig_text_view));

        transcript_grid.attach(&orig_label, 0, 0, 1, 1);
        transcript_grid.attach(&orig_scroller, 0, 1, 1, 1);

        // Right: Translated Dubbed Text
        let trans_label = gtk4::Label::new(Some("Dubbed Speech (Target)"));
        trans_label.add_css_class("heading");
        trans_label.set_halign(gtk4::Align::Start);

        let trans_scroller = gtk4::ScrolledWindow::new();
        trans_scroller.set_min_content_height(140);
        trans_scroller.set_propagate_natural_height(false);
        trans_scroller.add_css_class("card");

        let translated_buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
        let trans_text_view = gtk4::TextView::with_buffer(&translated_buffer);
        trans_text_view.set_editable(false);
        trans_text_view.set_cursor_visible(false);
        trans_text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        trans_text_view.set_margin_top(8);
        trans_text_view.set_margin_bottom(8);
        trans_text_view.set_margin_start(8);
        trans_text_view.set_margin_end(8);
        trans_scroller.set_child(Some(&trans_text_view));

        transcript_grid.attach(&trans_label, 1, 0, 1, 1);
        transcript_grid.attach(&trans_scroller, 1, 1, 1, 1);

        transcript_group.add(&transcript_grid);
        container.append(&transcript_group);

        clamp.set_child(Some(&container));
        scroller.set_child(Some(&clamp));

        Self {
            scroller,
            status_pill,
            status_desc,
            source_combo,
            apps_combo,
            refresh_apps_btn,
            model_combo,
            lang_combo,
            voice_combo,
            start_stop_btn,
            spinner,
            original_buffer,
            translated_buffer,
            detected_apps: Rc::new(RefCell::new(Vec::new())),
            is_running: Rc::new(RefCell::new(false)),
            cancel_token: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scroller
    }

    pub fn setup_events(
        &self,
        secret_store: Arc<dyn SecretStore>,
        settings: AppSettings,
        registry: Arc<LanguageRegistry>,
        parent_window: libadwaita::ApplicationWindow,
        toast_overlay: libadwaita::ToastOverlay,
    ) {
        let view_refresh = self.clone();

        // Refresh audio apps handler
        let refresh_fn = move || {
            let view = view_refresh.clone();
            glib::spawn_future_local(async move {
                let router = PactlAudioRouter::new();
                match router.list_sink_inputs().await {
                    Ok(apps) => {
                        let mut names = vec!["All Browser Windows (Auto Detect)".to_string()];
                        for app in &apps {
                            let title = app.media_name.as_deref().unwrap_or(&app.application_name);
                            names.push(format!("{} (#{})", title, app.sink_input_id));
                        }
                        let name_strs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                        let model = gtk4::StringList::new(&name_strs);
                        view.apps_combo.set_model(Some(&model));
                        *view.detected_apps.borrow_mut() = apps;
                    }
                    Err(e) => {
                        info!("Audio apps scan info: {}", e);
                    }
                }
            });
        };

        // Initial scan
        refresh_fn();

        self.refresh_apps_btn.connect_clicked(move |_| {
            refresh_fn();
        });

        // Start / Stop Toggle
        let view_toggle = self.clone();
        let store_click = secret_store.clone();
        let settings_click = settings.clone();
        let win_click = parent_window.clone();
        let toast_click = toast_overlay.clone();
        let reg_click = registry.clone();

        self.start_stop_btn.connect_clicked(move |_| {
            let is_active = *view_toggle.is_running.borrow();
            if is_active {
                // Stop session
                if let Some(token) = view_toggle.cancel_token.borrow().as_ref() {
                    token.cancel();
                }
                view_toggle.set_ui_stopped();
            } else {
                // Check API Key
                let api_key = match store_click.get_api_key() {
                    Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
                    _ => {
                        let toast = libadwaita::Toast::new(
                            "Please configure your Gemini API Key in Settings to start Live Dubbing",
                        );
                        toast_click.add_toast(toast);
                        SettingsDialog::show(&win_click, store_click.clone(), settings_click.clone());
                        return;
                    }
                };

                let client = GeminiClient::new(api_key);
                let live_streamer = Arc::new(GeminiLiveStreamer::new(client));
                let router = Arc::new(PactlAudioRouter::new());
                let orchestrator = Arc::new(LiveDubberOrchestrator::new(router, live_streamer));

                view_toggle.start_live_session(orchestrator, reg_click.clone());
            }
        });
    }

    fn set_ui_stopped(&self) {
        *self.is_running.borrow_mut() = false;
        self.start_stop_btn.set_label("Start Live Dubbing");
        self.start_stop_btn.remove_css_class("destructive-action");
        self.start_stop_btn.add_css_class("suggested-action");
        self.spinner.set_visible(false);
        self.spinner.stop();
        self.status_pill.set_label("Idle");
        self.status_pill.remove_css_class("error");
        self.status_pill.add_css_class("accent");
        self.status_desc.set_text("Live dubbing stopped.");
        self.source_combo.set_sensitive(true);
        self.apps_combo.set_sensitive(true);
        self.model_combo.set_sensitive(true);
        self.lang_combo.set_sensitive(true);
        self.voice_combo.set_sensitive(true);
    }

    fn set_ui_started(&self) {
        *self.is_running.borrow_mut() = true;
        self.start_stop_btn.set_label("Stop Live Dubbing");
        self.start_stop_btn.remove_css_class("suggested-action");
        self.start_stop_btn.add_css_class("destructive-action");
        self.spinner.set_visible(true);
        self.spinner.start();
        self.status_pill.set_label("🔴 Live Dubbing Active");
        self.status_pill.remove_css_class("accent");
        self.status_pill.add_css_class("error");
        self.source_combo.set_sensitive(false);
        self.apps_combo.set_sensitive(false);
        self.model_combo.set_sensitive(false);
        self.lang_combo.set_sensitive(false);
        self.voice_combo.set_sensitive(false);
    }

    fn start_live_session(
        &self,
        orchestrator: Arc<LiveDubberOrchestrator>,
        registry: Arc<LanguageRegistry>,
    ) {
        let cancel_token = CancellationToken::new();
        *self.cancel_token.borrow_mut() = Some(cancel_token.clone());
        self.set_ui_started();

        // 1. Resolve Model
        let model_idx = self.model_combo.selected();
        let model_choice = match model_idx {
            0 => LiveModelChoice::Gemini35LiveTranslate,
            1 => LiveModelChoice::Gemini31FlashLive,
            2 => LiveModelChoice::Gemini25FlashNativeAudio,
            3 => LiveModelChoice::Gemini35TranscribeLive,
            _ => LiveModelChoice::Gemini35LiveTranslate,
        };

        // 2. Resolve Audio Source Mode
        let source_idx = self.source_combo.selected();
        let source_mode = match source_idx {
            0 => AudioSourceMode::BrowserYouTube,
            1 => AudioSourceMode::SystemDesktop,
            2 => AudioSourceMode::Microphone,
            _ => AudioSourceMode::BrowserYouTube,
        };

        // 3. Resolve Target App
        let app_idx = self.apps_combo.selected() as usize;
        let target_app_id = if app_idx > 0 {
            let apps = self.detected_apps.borrow();
            apps.get(app_idx - 1).map(|a| a.sink_input_id)
        } else {
            None
        };

        // 4. Resolve Target Language
        let lang_idx = self.lang_combo.selected() as usize;
        let all_langs = registry.list();
        let (target_lang, target_lang_name) = if let Some(l) = all_langs.get(lang_idx) {
            (l.id.clone(), l.display_name.clone())
        } else {
            (LanguageId::new("id"), "Indonesian".to_string())
        };

        // 5. Resolve Voice Name
        let voice_idx = self.voice_combo.selected();
        let voice_name = match voice_idx {
            0 => "Aoede",
            1 => "Puck",
            2 => "Charon",
            3 => "Kore",
            4 => "Fenrir",
            _ => "Aoede",
        }
        .to_string();

        let options = LiveSessionOptions {
            model_choice,
            source_mode,
            target_app_id,
            target_language: target_lang,
            target_language_name: target_lang_name,
            voice_name,
        };

        // Clear previous transcripts
        self.original_buffer.set_text("");
        self.translated_buffer.set_text("");

        // Setup GLib channel for UI updates from async tasks
        let (status_tx, status_rx) = async_channel::unbounded::<LiveDubberStatus>();
        let (trans_tx, trans_rx) = async_channel::unbounded::<LiveTranscriptUpdate>();

        let view_status = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(status) = status_rx.recv().await {
                view_status.status_desc.set_text(status.display_status());
                if let LiveDubberStatus::Error(e) = &status {
                    view_status.status_pill.set_label("Error");
                    view_status.status_desc.set_text(e);
                    view_status.set_ui_stopped();
                    break;
                }
            }
        });

        let view_trans = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(update) = trans_rx.recv().await {
                if let Some(orig) = update.original_chunk {
                    let mut end = view_trans.original_buffer.end_iter();
                    view_trans
                        .original_buffer
                        .insert(&mut end, &format!("{} ", orig));
                }
                if let Some(dub) = update.translated_chunk {
                    let mut end = view_trans.translated_buffer.end_iter();
                    view_trans
                        .translated_buffer
                        .insert(&mut end, &format!("{} ", dub));
                }
            }
        });

        let view_done = self.clone();
        glib::spawn_future_local(async move {
            let res = orchestrator
                .start_session(
                    options,
                    cancel_token,
                    move |st| {
                        let _ = status_tx.send_blocking(st);
                    },
                    move |up| {
                        let _ = trans_tx.send_blocking(up);
                    },
                )
                .await;

            if let Err(e) = res {
                view_done.status_desc.set_text(&format!("Live Session Error: {}", e));
            }
            view_done.set_ui_stopped();
        });
    }
}
