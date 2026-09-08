use super::views::{
    DropzoneView, HistoryView, ProgressView, ResultView, ReviewTranscriptView, SettingsDialog,
    TtsStudioView,
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
        window.set_default_size(820, 680);

        let toast_overlay = libadwaita::ToastOverlay::new();
        let main_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        // HeaderBar
        let header = libadwaita::HeaderBar::new();

        // Main Stack (Root ViewStack for top-level navigation)
        let main_stack = libadwaita::ViewStack::new();
        main_stack.set_vexpand(true);
        main_stack.set_hexpand(true);

        // Dubbing Sub-Stack (dropzone -> progress -> result)
        let dubbing_stack = libadwaita::ViewStack::new();
        dubbing_stack.set_vexpand(true);
        dubbing_stack.set_hexpand(true);

        let dropzone_view = DropzoneView::new(&registry);
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

        // Top-level ViewSwitcher in HeaderBar for modern Adwaita workflow
        let view_switcher = libadwaita::ViewSwitcher::new();
        view_switcher.set_stack(Some(&main_stack));
        view_switcher.set_policy(libadwaita::ViewSwitcherPolicy::Wide);
        header.set_title_widget(Some(&view_switcher));

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
        main_box.append(&main_stack);
        toast_overlay.set_child(Some(&main_box));
        window.set_content(Some(&toast_overlay));

        // File Chooser Dialog integration for Dropzone
        let dropzone_file = dropzone_view.clone();
        let win_weak_file = window.downgrade();
        dropzone_view.connect_choose_file(move || {
            if let Some(win) = win_weak_file.upgrade() {
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Select Audio or Video File");

                let filter = gtk4::FileFilter::new();
                filter.set_name(Some(
                    "Media Files (*.mp3, *.wav, *.m4a, *.mp4, *.mkv, *.mov, *.webm)",
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
        let dubbing_stack_cancel = dubbing_stack.clone();
        progress_view.connect_cancel(move || {
            if let Some(ref token) = *token_clone.borrow() {
                token.cancel();
            }
            dubbing_stack_cancel.set_visible_child_name("dropzone");
        });

        // Connect "New Translation" button in ResultView
        let dubbing_stack_new = dubbing_stack.clone();
        result_view.connect_new_clicked(move || {
            dubbing_stack_new.set_visible_child_name("dropzone");
        });

        // Connect History Page Refresh and Back button
        let hist_refresh = history_view.clone();
        let repo_refresh = job_repo.clone();
        main_stack.connect_visible_child_name_notify(move |stack| {
            if stack.visible_child_name().as_deref() == Some("history") {
                let h = hist_refresh.clone();
                let r = repo_refresh.clone();
                glib::spawn_future_local(async move {
                    h.refresh(r).await;
                });
            }
        });

        let main_stack_hist_back = main_stack.clone();
        history_view.connect_back_clicked(move || {
            main_stack_hist_back.set_visible_child_name("dubbing");
        });

        // Initial history load on startup
        let hist_init = history_view.clone();
        let repo_init = job_repo.clone();
        glib::spawn_future_local(async move {
            hist_init.refresh(repo_init).await;
        });

        // Setup Communication Channel from Tokio to GTK
        let (sender, receiver) = async_channel::unbounded::<UiMessage>();

        let dubbing_stack_msg = dubbing_stack.clone();
        let progress_clone_msg = progress_view.clone();
        let review_clone_msg = review_view.clone();
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
                    UiMessage::ProgressDetail(detail) => {
                        progress_clone_msg.update_detail(&detail);
                    }
                    UiMessage::ReviewCheckpoint(req) => {
                        review_clone_msg.populate(req.translated, req.resume_sender);
                        dubbing_stack_msg.set_visible_child_name("review");
                    }
                    UiMessage::Success(artifact, src_lang, tgt_lang) => {
                        result_clone_msg.set_result(&artifact, &src_lang, &tgt_lang);
                        dubbing_stack_msg.set_visible_child_name("result");
                        let toast = libadwaita::Toast::new("Translation finished successfully!");
                        toast_clone_msg.add_toast(toast);
                        let h = hist_succ.clone();
                        let r = repo_succ.clone();
                        glib::spawn_future_local(async move {
                            h.refresh(r).await;
                        });
                    }
                    UiMessage::Error(err) => {
                        dubbing_stack_msg.set_visible_child_name("dropzone");
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
        let dubbing_stack_exec = dubbing_stack.clone();
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
            let engine = dropzone_exec.selected_engine();
            let duck_audio = dropzone_exec.is_ducking_enabled();
            let review_transcript = dropzone_exec.is_review_enabled();

            if let Err(err_msg) = registry_exec.validate_pair(&src_lang, &tgt_lang) {
                let toast = libadwaita::Toast::new(&err_msg);
                toast_exec.add_toast(toast);
                return;
            }

            // Transition dubbing view to progress screen
            dubbing_stack_exec.set_visible_child_name("progress");

            let cancel_token = CancellationToken::new();
            *token_exec.borrow_mut() = Some(cancel_token.clone());

            let sender_clone = sender.clone();
            let audio_engine_bg = audio_engine_exec.clone();
            let job_repo_bg = job_repo_exec.clone();
            let settings_bg = settings_exec.clone();

            let (review_tx, review_rx) = async_channel::unbounded();
            let pipeline_options = PipelineOptions {
                tone,
                voice_config,
                export_subtitles: true,
                engine,
                duck_audio,
                review_transcript,
                review_channel: if review_transcript {
                    Some(review_tx)
                } else {
                    None
                },
            };

            if review_transcript {
                let sender_rev = sender_clone.clone();
                tokio::spawn(async move {
                    while let Ok(req) = review_rx.recv().await {
                        let _ = sender_rev.send(UiMessage::ReviewCheckpoint(req)).await;
                    }
                });
            }

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

                let sender_retry = sender_clone.clone();
                let status_cb: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |msg: &str| {
                    let _ = sender_retry.send_blocking(UiMessage::ProgressDetail(msg.to_string()));
                });

                let gemini_client = GeminiClient::new(api_key).with_status_callback(status_cb);
                let transcriber = Arc::new(GeminiTranscriber::new(
                    gemini_client.clone(),
                    settings_bg.models.transcriber.clone(),
                ));
                let translator = Arc::new(GeminiTranslator::new(
                    gemini_client.clone(),
                    settings_bg.models.translator.clone(),
                ));
                let synthesizer = Arc::new(GeminiSynthesizer::new(
                    gemini_client.clone(),
                    settings_bg.models.tts.clone(),
                ));
                let live_translator = Arc::new(GeminiLiveTranslator::new(
                    gemini_client,
                    settings_bg.models.live_translate.clone(),
                ));

                let orchestrator = PipelineOrchestrator::with_settings(
                    transcriber,
                    translator,
                    synthesizer,
                    audio_engine_bg,
                    job_repo_bg,
                    &settings_bg,
                )
                .with_live_translator(live_translator);

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

        // Connect TTS Studio "Generate Speech" clicked
        let tts_exec = tts_view.clone();
        let secret_store_tts = secret_store.clone();
        let settings_tts = settings.clone();
        let toast_tts = toast_overlay.clone();
        let win_weak_tts = window.downgrade();

        tts_view.connect_generate_clicked(move || {
            let api_key = match secret_store_tts.get_api_key() {
                Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
                _ => {
                    let toast =
                        libadwaita::Toast::new("Please configure your Gemini API Key in Settings");
                    toast_tts.add_toast(toast);
                    if let Some(win) = win_weak_tts.upgrade() {
                        SettingsDialog::show(&win, secret_store_tts.clone(), settings_tts.clone());
                    }
                    return;
                }
            };

            let text = tts_exec.text().trim().to_string();
            if text.is_empty() {
                let toast = libadwaita::Toast::new("Please enter text to synthesize");
                toast_tts.add_toast(toast);
                return;
            }

            let voice_profile = tts_exec.build_voice_profile();
            let style_instruction = tts_exec.selected_style_instruction();
            let speed = tts_exec.selected_speed();

            tts_exec.set_generating(true, "Synthesizing expressive speech with Gemini...");

            let tts_ui = tts_exec.clone();
            let toast_ui = toast_tts.clone();
            let tts_model_name = settings_tts.models.tts.clone();

            glib::spawn_future_local(async move {
                let tts_dir = dirs::data_local_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("audiodub")
                    .join("tts");
                let _ = tokio::fs::create_dir_all(&tts_dir).await;

                let filename = format!(
                    "tts_{}_{}.wav",
                    chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                    &uuid::Uuid::new_v4().to_string()[..8]
                );
                let out_wav_path = tts_dir.join(&filename);

                let (tts_status_tx, tts_status_rx) = async_channel::unbounded::<String>();
                let tts_ui_status = tts_ui.clone();
                glib::spawn_future_local(async move {
                    while let Ok(msg) = tts_status_rx.recv().await {
                        tts_ui_status.set_status(&msg);
                    }
                });

                let status_cb: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |msg: &str| {
                    let _ = tts_status_tx.send_blocking(msg.to_string());
                });

                let client = GeminiClient::new(api_key).with_status_callback(status_cb);
                let synth = GeminiSynthesizer::new(client, tts_model_name);

                use crate::application::ports::SpeechSynthesizer;
                let synth_res = synth
                    .synthesize_text(
                        &text,
                        &voice_profile,
                        style_instruction.as_deref(),
                        &out_wav_path,
                    )
                    .await;

                match synth_res {
                    Ok(seg) => {
                        let final_path = if (speed - 1.0).abs() > 0.05 {
                            let stretched_filename = format!(
                                "tts_{}_{}_speed.wav",
                                chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                                &uuid::Uuid::new_v4().to_string()[..8]
                            );
                            let stretched_path = tts_dir.join(&stretched_filename);
                            let in_p = out_wav_path.clone();
                            let out_p = stretched_path.clone();
                            let stretch_res = tokio::task::spawn_blocking(move || {
                                crate::infrastructure::ffmpeg::FfmpegAligner::time_stretch(
                                    &in_p,
                                    &out_p,
                                    speed as f64,
                                )
                            })
                            .await;

                            match stretch_res {
                                Ok(Ok(())) => stretched_path,
                                _ => out_wav_path,
                            }
                        } else {
                            out_wav_path
                        };

                        let p = final_path.clone();
                        let duration_ms = match tokio::task::spawn_blocking(move || {
                            crate::infrastructure::ffmpeg::FfprobeInspector::probe(&p)
                        })
                        .await
                        {
                            Ok(Ok(meta)) => meta.duration_ms,
                            _ => seg.duration_ms,
                        };

                        tts_ui.set_generating(false, "Speech generated successfully!");
                        tts_ui.set_result(final_path, duration_ms);
                        let toast = libadwaita::Toast::new("Speech synthesis completed!");
                        toast_ui.add_toast(toast);
                    }
                    Err(e) => {
                        tts_ui.set_generating(false, "");
                        let toast = libadwaita::Toast::new(&format!("TTS Error: {}", e));
                        toast.set_timeout(6);
                        toast_ui.add_toast(toast);
                    }
                }
            });
        });

        // Connect TTS Studio "Export MP3..." clicked
        let win_weak_export = window.downgrade();
        let toast_export = toast_overlay.clone();
        tts_view.connect_export_clicked(move |source_path| {
            if let Some(win) = win_weak_export.upgrade() {
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Export Synthesized Audio");
                dialog.set_initial_name(Some("synthesized_speech.mp3"));

                let filter = gtk4::FileFilter::new();
                filter.set_name(Some("Audio Files (*.mp3, *.wav)"));
                filter.add_pattern("*.mp3");
                filter.add_pattern("*.wav");
                let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
                filters.append(&filter);
                dialog.set_filters(Some(&filters));

                let toast_cb = toast_export.clone();
                dialog.save(Some(&win), gtk4::gio::Cancellable::NONE, move |res| {
                    if let Ok(file) = res {
                        if let Some(dest_path) = file.path() {
                            let src = source_path.clone();
                            let dst = dest_path.clone();
                            std::thread::spawn(move || {
                                let _ = std::process::Command::new("ffmpeg")
                                    .arg("-y")
                                    .arg("-i")
                                    .arg(&src)
                                    .arg("-c:a")
                                    .arg("libmp3lame")
                                    .arg("-b:a")
                                    .arg("192k")
                                    .arg(&dst)
                                    .output();
                            });
                            let toast = libadwaita::Toast::new(&format!(
                                "Saved to: {}",
                                dest_path.file_name().unwrap_or_default().to_string_lossy()
                            ));
                            toast_cb.add_toast(toast);
                        }
                    }
                });
            }
        });

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
