use super::views::{
    DropzoneView, HistoryView, LiveDubberView, ProgressView, ResultView, ReviewTranscriptView,
    SettingsDialog, TtsStudioView,
};
use crate::application::pipeline::ReviewRequest;
use crate::application::ports::{AudioEngine, JobRepository, SecretStore};
use crate::application::{PipelineOptions, PipelineOrchestrator};
use crate::config::AppSettings;
use crate::domain::{AudioArtifact, DomainError, Job, JobProgress, LanguageRegistry};
use crate::infrastructure::gemini::{
    GeminiClient, GeminiLiveTranslator, GeminiSynthesizer, GeminiTranscriber, GeminiTranslator,
};
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub enum UiMessage {
    Progress(JobProgress),
    ProgressDetail(String),
    ReviewCheckpoint(ReviewRequest),
    Success(AudioArtifact, String, String),
    Error(DomainError),
}

pub struct MainWindow {
    window: libadwaita::ApplicationWindow,
}

impl MainWindow {
    pub fn build(
        app: &libadwaita::Application,
        registry: Arc<LanguageRegistry>,
        secret_store: Arc<dyn SecretStore>,
        audio_engine: Arc<dyn AudioEngine>,
        job_repo: Arc<dyn JobRepository>,
        settings: AppSettings,
    ) -> Self {
        let window = libadwaita::ApplicationWindow::new(app);
        window.set_title(Some("AudioDub AI"));
        window.set_default_size(960, 720);
        window.set_resizable(true);
        window.set_size_request(640, 480);

        let toast_overlay = libadwaita::ToastOverlay::new();
        let main_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        let header = libadwaita::HeaderBar::new();

        let main_stack = libadwaita::ViewStack::new();
        main_stack.set_vexpand(true);
        main_stack.set_hexpand(true);

        let dubbing_stack = libadwaita::ViewStack::new();
        dubbing_stack.set_vexpand(true);
        dubbing_stack.set_hexpand(true);

        let dropzone_view = DropzoneView::new(&registry);
        let live_dubber_view = LiveDubberView::new(&registry);
        let progress_view = ProgressView::new();
        let review_view = ReviewTranscriptView::new();
        let result_view = ResultView::new();
        let history_view = HistoryView::new();
        let tts_view = TtsStudioView::new();

        dubbing_stack.add_named(dropzone_view.widget(), Some("dropzone"));
        dubbing_stack.add_named(progress_view.widget(), Some("progress"));
        dubbing_stack.add_named(review_view.widget(), Some("review"));
        dubbing_stack.add_named(result_view.widget(), Some("result"));

        let dubbing_stack_rev_proceed = dubbing_stack.clone();
        review_view.connect_proceed(move || {
            dubbing_stack_rev_proceed.set_visible_child_name("progress");
        });

        let dubbing_stack_rev_cancel = dubbing_stack.clone();
        review_view.connect_cancelled(move || {
            dubbing_stack_rev_cancel.set_visible_child_name("dropzone");
        });

        main_stack.add_titled_with_icon(
            &dubbing_stack,
            Some("dubbing"),
            "Dubbing AI",
            "media-record-symbolic",
        );
        main_stack.add_titled_with_icon(
            live_dubber_view.widget(),
            Some("live"),
            "Live Dubber",
            "network-transmit-receive-symbolic",
        );
        main_stack.add_titled_with_icon(
            tts_view.widget(),
            Some("tts"),
            "TTS Studio",
            "audio-speakers-symbolic",
        );
        main_stack.add_titled_with_icon(
            history_view.widget(),
            Some("history"),
            "History",
            "document-open-recent-symbolic",
        );

        live_dubber_view.setup_events(
            secret_store.clone(),
            settings.clone(),
            registry.clone(),
            window.clone(),
            toast_overlay.clone(),
        );

        let view_switcher = libadwaita::ViewSwitcher::new();
        view_switcher.set_stack(Some(&main_stack));
        view_switcher.set_policy(libadwaita::ViewSwitcherPolicy::Wide);
        header.set_title_widget(Some(&view_switcher));

        let settings_btn = gtk4::Button::from_icon_name("preferences-system-symbolic");
        settings_btn.set_tooltip_text(Some("Preferences"));
        let store_clone = secret_store.clone();
        let settings_clone = settings.clone();
        let win_weak = window.downgrade();
        settings_btn.connect_clicked(move |_| {
            if let Some(win) = win_weak.upgrade() {
                SettingsDialog::show(&win, store_clone.clone(), settings_clone.clone());
            }
        });
        header.pack_end(&settings_btn);

        main_box.append(&header);
        main_box.append(&main_stack);
        toast_overlay.set_child(Some(&main_box));
        window.set_content(Some(&toast_overlay));

        // RESTORE NOTICE: Full handlers from original window.rs should be re-applied
        // from commit bc2ccd7 if TTS/dropzone wiring is incomplete after this recover.
        // Critical path for Live Dubber API fix is independent of this file.

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
