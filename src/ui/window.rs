use super::views::{DropzoneView, HistoryView, ProgressView, ResultView, SettingsDialog};
use crate::application::ports::{AudioEngine, JobRepository, SecretStore};
use crate::application::{PipelineOptions, PipelineOrchestrator};
use crate::config::AppSettings;
use crate::domain::{AudioArtifact, DomainError, Job, JobProgress, LanguageRegistry};
use crate::infrastructure::gemini::{
    GeminiClient, GeminiSynthesizer, GeminiTranscriber, GeminiTranslator,
};
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub enum UiMessage {
    Progress(JobProgress),
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
        window.set_default_size(800, 680);

        let toast_overlay = libadwaita::ToastOverlay::new();
        let main_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        // HeaderBar
        let header = libadwaita::HeaderBar::new();
        let title = libadwaita::WindowTitle::new("AudioDub AI", "Desktop Voice Dubbing");
        header.set_title_widget(Some(&title));

        // History Toggle Button
        let history_btn = gtk4::Button::from_icon_name("document-open-recent-symbolic");
        history_btn.set_tooltip_text(Some("Translation History"));
        header.pack_start(&history_btn);

        // Settings Button
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

        // View Stack
        let view_stack = libadwaita::ViewStack::new();

        let dropzone_view = DropzoneView::new(&registry);
        let progress_view = ProgressView::new();
        let result_view = ResultView::new();
        let history_view = HistoryView::new();

        view_stack.add_titled(dropzone_view.widget(), Some("dropzone"), "Translate");
        view_stack.add_titled(progress_view.widget(), Some("progress"), "Progress");
        view_stack.add_titled(result_view.widget(), Some("result"), "Result");
        view_stack.add_titled(history_view.widget(), Some("history"), "History");

        main_box.append(&view_stack);
        toast_overlay.set_child(Some(&main_box));
        window.set_content(Some(&toast_overlay));

        // File Chooser Dialog integration
        let dropzone_file = dropzone_view.clone();
        let win_weak_file = window.downgrade();
        dropzone_view.connect_choose_file(move || {
            if let Some(win) = win_weak_file.upgrade() {
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Select Audio File");

                let filter = gtk4::FileFilter::new();
                filter.set_name(Some(
                    "Audio & Video Files (*.mp3, *.wav, *.m4a, *.mp4, *.mkv, *.mov, *.webm)",
                ));
                filter.add_mime_type("audio/*");
                filter.add_mime_type("video/*");
                filter.add_pattern("*.mp3");
                filter.add_pattern("*.wav");
                filter.add_pattern("*.m4a");
                filter.add_pattern("*.flac");
                filter.add_pattern("*.ogg");
                filter.add_pattern("*.mp4");
                filter.add_pattern("*.mkv");
                filter.add_pattern("*.mov");
                filter.add_pattern("*.webm");

                let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
                filters.append(&filter);
                dialog.set_filters(Some(&filters));

                let dropzone_cb = dropzone_file.clone();
                dialog.open(Some(&win), gtk4::gio::Cancellable::NONE, move |res| {
                    if let Ok(file) = res {
                        if let Some(path) = file.path() {
                            dropzone_cb.set_selected_file(path);
                        }
                    }
                });
            }
        });

        // Drag & drop target
        let drop_target =
            gtk4::DropTarget::new(gtk4::gio::File::static_type(), gtk4::gdk::DragAction::COPY);
        let dropzone_dnd = dropzone_view.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            if let Ok(file) = value.get::<gtk4::gio::File>() {
                if let Some(path) = file.path() {
                    dropzone_dnd.set_selected_file(path);
                    return true;
                }
            }
            false
        });
        dropzone_view.widget().add_controller(drop_target);

        // Cancel token cell
        let current_cancel_token =
            std::rc::Rc::new(std::cell::RefCell::new(None::<CancellationToken>));

        // Connect Progress Cancel button
        let token_clone = current_cancel_token.clone();
        progress_view.connect_cancel(move || {
            if let Some(ref token) = *token_clone.borrow() {
                token.cancel();
            }
        });

        // Connect "New Translation" button in ResultView
        let stack_clone_new = view_stack.clone();
        result_view.connect_new_clicked(move || {
            stack_clone_new.set_visible_child_name("dropzone");
        });

        // Connect History Toggle and Back
        let stack_clone_hist = view_stack.clone();
        let hist_clone = history_view.clone();
        let repo_clone_hist = job_repo.clone();
        history_btn.connect_clicked(move |_| {
            let current = stack_clone_hist.visible_child_name();
            if current.as_deref() == Some("history") {
                stack_clone_hist.set_visible_child_name("dropzone");
            } else {
                stack_clone_hist.set_visible_child_name("history");
                let h = hist_clone.clone();
                let r = repo_clone_hist.clone();
                glib::spawn_future_local(async move {
                    h.refresh(r).await;
                });
            }
        });

        let stack_clone_hist_back = view_stack.clone();
        history_view.connect_back_clicked(move || {
            stack_clone_hist_back.set_visible_child_name("dropzone");
        });

        // Initial history load on startup
        let hist_init = history_view.clone();
        let repo_init = job_repo.clone();
        glib::spawn_future_local(async move {
            hist_init.refresh(repo_init).await;
        });

        // Setup Communication Channel from Tokio to GTK
        let (sender, receiver) = async_channel::unbounded::<UiMessage>();

        let stack_clone_msg = view_stack.clone();
        let progress_clone_msg = progress_view.clone();
        let result_clone_msg = result_view.clone();
        let toast_clone_msg = toast_overlay.clone();
        let hist_succ = history_view.clone();
        let repo_succ = job_repo.clone();

        glib::spawn_future_local(async move {
            while let Ok(msg) = receiver.recv().await {
                match msg {
                    UiMessage::Progress(progress) => {
                        progress_clone_msg.update_progress(&progress);
                    }
                    UiMessage::Success(artifact, src_lang, tgt_lang) => {
                        result_clone_msg.set_result(&artifact, &src_lang, &tgt_lang);
                        stack_clone_msg.set_visible_child_name("result");
                        let toast = libadwaita::Toast::new("Translation finished successfully!");
                        toast_clone_msg.add_toast(toast);
                        let h = hist_succ.clone();
                        let r = repo_succ.clone();
                        glib::spawn_future_local(async move {
                            h.refresh(r).await;
                        });
                    }
                    UiMessage::Error(err) => {
                        stack_clone_msg.set_visible_child_name("dropzone");
                        let toast =
                            libadwaita::Toast::new(&format!("{}: {}", err.human_title(), err));
                        toast.set_timeout(6);
                        toast_clone_msg.add_toast(toast);
                    }
                }
            }
        });

        // Connect "Translate & Dub" clicked
        let dropzone_exec = dropzone_view.clone();
        let stack_exec = view_stack.clone();
        let secret_store_exec = secret_store.clone();
        let audio_engine_exec = audio_engine.clone();
        let job_repo_exec = job_repo.clone();
        let settings_exec = settings.clone();
        let registry_exec = registry.clone();
        let token_exec = current_cancel_token.clone();
        let toast_exec = toast_overlay.clone();
        let win_weak_exec = window.downgrade();

        dropzone_view.connect_translate_clicked(move || {
            let api_key = match secret_store_exec.get_api_key() {
                Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
                _ => {
                    let toast =
                        libadwaita::Toast::new("Please configure your Gemini API Key in Settings");
                    toast_exec.add_toast(toast);
                    if let Some(win) = win_weak_exec.upgrade() {
                        SettingsDialog::show(
                            &win,
                            secret_store_exec.clone(),
                            settings_exec.clone(),
                        );
                    }
                    return;
                }
            };

            let input_path = match dropzone_exec.selected_path() {
                Some(p) => p,
                None => {
                    let toast = libadwaita::Toast::new("Please select an audio file first");
                    toast_exec.add_toast(toast);
                    return;
                }
            };

            let src_lang = dropzone_exec.selected_source_language();
            let tgt_lang = dropzone_exec.selected_target_language();
            let tone = dropzone_exec.selected_tone();
            let voice_config = dropzone_exec.selected_voice_config();

            if let Err(err_msg) = registry_exec.validate_pair(&src_lang, &tgt_lang) {
                let toast = libadwaita::Toast::new(&err_msg);
                toast_exec.add_toast(toast);
                return;
            }

            // Transition UI to progress screen
            stack_exec.set_visible_child_name("progress");

            let cancel_token = CancellationToken::new();
            *token_exec.borrow_mut() = Some(cancel_token.clone());

            let sender_clone = sender.clone();
            let audio_engine_bg = audio_engine_exec.clone();
            let job_repo_bg = job_repo_exec.clone();
            let settings_bg = settings_exec.clone();

            let pipeline_options = PipelineOptions {
                tone,
                voice_config,
                export_subtitles: true,
            };

            // Spawn asynchronous job execution in Tokio background thread
            tokio::spawn(async move {
                let doc = match audio_engine_bg
                    .inspect_and_validate(&input_path, settings_bg.audio.max_file_size_bytes)
                    .await
                {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = sender_clone.send(UiMessage::Error(e)).await;
                        return;
                    }
                };

                let job = Job::new(doc, src_lang.clone(), tgt_lang.clone());

                let gemini_client = GeminiClient::new(api_key);
                let transcriber = Arc::new(GeminiTranscriber::new(
                    gemini_client.clone(),
                    settings_bg.models.transcriber.clone(),
                ));
                let translator = Arc::new(GeminiTranslator::new(
                    gemini_client.clone(),
                    settings_bg.models.translator.clone(),
                ));
                let synthesizer = Arc::new(GeminiSynthesizer::new(
                    gemini_client,
                    settings_bg.models.tts.clone(),
                ));

                let orchestrator = PipelineOrchestrator::with_settings(
                    transcriber,
                    translator,
                    synthesizer,
                    audio_engine_bg,
                    job_repo_bg,
                    &settings_bg,
                );

                let sender_progress = sender_clone.clone();
                let on_progress = move |updated_job: &Job| {
                    let _ = sender_progress
                        .send_blocking(UiMessage::Progress(updated_job.progress.clone()));
                };

                match orchestrator
                    .run_job_with_options(job, pipeline_options, cancel_token, on_progress)
                    .await
                {
                    Ok(artifact) => {
                        let _ = sender_clone
                            .send(UiMessage::Success(
                                artifact,
                                src_lang.as_str().to_string(),
                                tgt_lang.as_str().to_string(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        let _ = sender_clone.send(UiMessage::Error(e)).await;
                    }
                }
            });
        });

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
