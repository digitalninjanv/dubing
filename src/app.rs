use crate::config::AppSettings;
use crate::domain::LanguageRegistry;
use crate::infrastructure::ffmpeg::FfmpegAudioEngine;
use crate::infrastructure::filesystem::{AppPaths, CleanupManager, FileJobRepository};
use crate::infrastructure::logging::init_logging;
use crate::infrastructure::secrets::StandardSecretStore;
use crate::ui::MainWindow;
use libadwaita::prelude::*;
use std::sync::Arc;

pub const APP_ID: &str = "io.github.digitalninjanv.AudioDub";

pub struct AudioDubApp;

impl AudioDubApp {
    pub fn run() -> glib::ExitCode {
        init_logging();
        let _ = AppPaths::ensure_dirs();
        let settings = AppSettings::load();
        CleanupManager::startup_reconciliation();
        CleanupManager::retention_sweep(settings.job_retention_days);

        let app = libadwaita::Application::builder()
            .application_id(APP_ID)
            .build();

        app.connect_activate(move |application| {
            let registry = Arc::new(LanguageRegistry::standard());
            let secret_store = Arc::new(StandardSecretStore::new());
            let audio_engine = Arc::new(FfmpegAudioEngine::new());
            let job_repo = Arc::new(FileJobRepository::new());
            let settings = settings.clone();

            let window = MainWindow::build(
                application,
                registry,
                secret_store,
                audio_engine,
                job_repo,
                settings,
            );

            window.present();
        });

        app.run()
    }
}
