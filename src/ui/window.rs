use super::views::{
    DropzoneView, HistoryView, ProgressView, ResultView, ReviewTranscriptView, SettingsDialog,
    TtsStudioView,
};
use crate::application::pipeline::ReviewRequest;
use crate::application::ports::{AudioEngine, JobRepository, SecretStore, SpeechSynthesizer};
use crate::application::{PipelineOptions, PipelineOrchestrator};
use crate::config::AppSettings;
use crate::domain::{AudioArtifact, DomainError, Job, JobProgress, LanguageRegistry};
use crate::infrastructure::ffmpeg::FfmpegAligner;
use crate::infrastructure::filesystem::AppPaths;
use crate::infrastructure::gemini::{
    GeminiClient, GeminiSynthesizer, GeminiTranscriber, GeminiTranslator,
};
use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
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
            let duck_audio = dropzone_exec.is_ducking_enabled();
            let review_transcript = dropzone_exec.is_review_enabled();

            if let Err(err_msg) = registry_exec.validate_pair(&src_lang, &tgt_lang) {
                let toast = libadwaita::Toast::new(&err_msg);
                toast_exec.add_toast(toast);
                return;
            }

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

                let gemini_client = GeminiClient::new(api_key)
                    .with_status_callback(status_cb)
                    .with_cancel_token(cancel_token.clone());
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

        // TTS Studio wiring: Generate / Play / Export.
        {
            let tts_exec = tts_view.clone();
            let store_tts = secret_store.clone();
            let settings_tts = settings.clone();
            let toast_tts = toast_overlay.clone();
            let win_tts = window.downgrade();
            let tts_cancel_exec: Rc<RefCell<Option<CancellationToken>>> =
                Rc::new(RefCell::new(None));

            tts_view.connect_generate_clicked(move || {
                let text = tts_exec.text();
                if text.trim().is_empty() {
                    toast_tts.add_toast(libadwaita::Toast::new(
                        "Enter some text to synthesize first",
                    ));
                    return;
                }
                let api_key = match store_tts.get_api_key() {
                    Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
                    _ => {
                        toast_tts.add_toast(libadwaita::Toast::new(
                            "Please configure your Gemini API Key in Settings",
                        ));
                        if let Some(win) = win_tts.upgrade() {
                            SettingsDialog::show(&win, store_tts.clone(), settings_tts.clone());
                        }
                        return;
                    }
                };

                let voice = tts_exec.build_voice_profile();
                let speed = voice.speed;
                let style = voice.style.clone();
                let view_bg = tts_exec.clone();
                let settings_bg = settings_tts.clone();
                // Cancel any in-flight generation before starting a new one.
                if let Some(old) = tts_cancel_exec.borrow().as_ref() {
                    old.cancel();
                }
                let tts_token = CancellationToken::new();
                *tts_cancel_exec.borrow_mut() = Some(tts_token.clone());
                view_bg.set_generating(true, "Synthesizing speech...");

                let (tx, rx) = async_channel::bounded::<Result<(PathBuf, u64), String>>(1);
                tokio::spawn(async move {
                    let res: Result<(PathBuf, u64), String> = async {
                        let client = GeminiClient::new(api_key).with_cancel_token(tts_token);
                        let synth = GeminiSynthesizer::new(client, settings_bg.models.tts.clone());
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let out = AppPaths::outputs_dir().join(format!(
                            "tts_studio_{}_{}.wav",
                            ts,
                            std::process::id()
                        ));
                        if let Some(p) = out.parent() {
                            let _ = std::fs::create_dir_all(p);
                        }
                        let seg = synth
                            .synthesize_text(&text, &voice, style.as_deref(), &out)
                            .await
                            .map_err(|e| e.to_string())?;
                        if (speed - 1.0).abs() > 0.05 {
                            let stretched =
                                out.with_file_name(format!("tts_studio_{}_x{}.wav", ts, speed));
                            let src = out.clone();
                            let dst = stretched.clone();
                            tokio::task::spawn_blocking(move || {
                                FfmpegAligner::time_stretch(&src, &dst, speed as f64)
                            })
                            .await
                            .map_err(|e| format!("stretch task: {}", e))?
                            .map_err(|e| e.to_string())?;
                            Ok((stretched, seg.duration_ms))
                        } else {
                            Ok((out, seg.duration_ms))
                        }
                    }
                    .await;
                    let _ = tx.send(res).await;
                });

                let view_done = tts_exec.clone();
                let toast_done = toast_tts.clone();
                glib::spawn_future_local(async move {
                    if let Ok(res) = rx.recv().await {
                        match res {
                            Ok((path, dur)) => {
                                view_done.set_generating(false, "Done");
                                view_done.set_result(path, dur);
                                toast_done.add_toast(libadwaita::Toast::new(
                                    "Speech generated — press play to preview",
                                ));
                            }
                            Err(e) => {
                                view_done.set_generating(false, "Ready");
                                view_done.set_status(&format!("Failed: {}", e));
                                toast_done.add_toast(libadwaita::Toast::new(&format!(
                                    "TTS failed: {}",
                                    e
                                )));
                            }
                        }
                    }
                });
            });

            // Play: ffplay -> pw-play -> xdg-open, loud toast if all fail.
            // Tracks the player process so a new play kills the previous one.
            let toast_play = toast_overlay.clone();
            let player_handle: Rc<RefCell<Option<std::process::Child>>> =
                Rc::new(RefCell::new(None));
            tts_view.connect_play_clicked(move |path| {
                if let Some(mut old) = player_handle.borrow_mut().take() {
                    let _ = old.kill();
                    let _ = old.wait();
                }
                let arg = path.to_string_lossy().to_string();
                let attempts: &[&[&str]] = &[
                    &["ffplay", "-nodisp", "-autoexit", "-loglevel", "error"],
                    &["pw-play"],
                ];
                for attempt in attempts {
                    let mut cmd = std::process::Command::new(attempt[0]);
                    for a in &attempt[1..] {
                        cmd.arg(a);
                    }
                    match cmd.arg(&arg).stdin(std::process::Stdio::null()).spawn() {
                        Ok(child) => {
                            *player_handle.borrow_mut() = Some(child);
                            return;
                        }
                        Err(_) => continue,
                    }
                }
                match std::process::Command::new("xdg-open").arg(&arg).spawn() {
                    Ok(_) => {}
                    Err(e) => {
                        toast_play.add_toast(libadwaita::Toast::new(&format!(
                            "Cannot play audio (install ffplay): {}",
                            e
                        )));
                    }
                }
            });

            // Export: WAV preview -> MP3 in outputs dir (off the UI thread).
            let toast_exp = toast_overlay.clone();
            tts_view.connect_export_clicked(move |path| {
                let mp3 = path.with_extension("mp3");
                let (tx, rx) = async_channel::bounded::<Result<String, String>>(1);
                std::thread::spawn(move || {
                    let res = match std::process::Command::new("ffmpeg")
                        .arg("-y")
                        .arg("-i")
                        .arg(&path)
                        .arg("-c:a")
                        .arg("libmp3lame")
                        .arg("-b:a")
                        .arg("192k")
                        .arg(&mp3)
                        .output()
                    {
                        Ok(out) if out.status.success() => Ok(mp3.display().to_string()),
                        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = tx.send_blocking(res);
                });
                let toast_done = toast_exp.clone();
                glib::spawn_future_local(async move {
                    if let Ok(res) = rx.recv().await {
                        match res {
                            Ok(disp) => {
                                toast_done.add_toast(libadwaita::Toast::new(&format!(
                                    "Exported MP3: {}",
                                    disp
                                )));
                            }
                            Err(e) => {
                                toast_done.add_toast(libadwaita::Toast::new(&format!(
                                    "Export failed: {}",
                                    e
                                )));
                            }
                        }
                    }
                });
            });
        }

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
