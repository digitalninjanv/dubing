use audiodub::app::AudioDubApp;
use audiodub::application::ports::{AudioEngine, SecretStore};
use audiodub::application::PipelineOrchestrator;
use audiodub::config::AppSettings;
use audiodub::domain::{LanguageId, LanguageRegistry};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use audiodub::infrastructure::filesystem::FileJobRepository;
use audiodub::infrastructure::gemini::{
    GeminiClient, GeminiSynthesizer, GeminiTranscriber, GeminiTranslator,
};
use audiodub::infrastructure::secrets::StandardSecretStore;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> glib::ExitCode {
    let args: Vec<String> = env::args().collect();

    // Check if CLI mode was requested
    if args.len() > 1 && (args[1] == "translate" || args[1] == "--help" || args[1] == "-h") {
        if args[1] == "--help" || args[1] == "-h" {
            println!("AudioDub AI — Spoken Audio Translation & Dubbing");
            println!("\nUsage:");
            println!(
                "  audiodub                                      Launch GTK4 / Libadwaita GUI"
            );
            println!("  audiodub translate <input> --target <lang>    Translate in CLI mode");
            println!("\nOptions:");
            println!("  --target <lang>     Target language code (e.g., en, id, ja, es, ko)");
            println!("  --source <lang>     Source language code (default: auto)");
            println!("  --output <path>     Output MP3 file path");
            return glib::ExitCode::SUCCESS;
        }

        if args[1] == "translate" {
            if let Err(e) = run_cli(&args[2..]).await {
                eprintln!("\nError: {}", e);
                return glib::ExitCode::FAILURE;
            }
            return glib::ExitCode::SUCCESS;
        }
    }

    // Default: Launch Native Desktop GUI
    AudioDubApp::run()
}

async fn run_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err(
            "Input file path required. Usage: audiodub translate <input.mp3> --target <lang>"
                .into(),
        );
    }

    let input_path = PathBuf::from(&args[0]);
    let mut target_lang_str = "en".to_string();
    let mut source_lang_str = "auto".to_string();
    let mut output_path_opt: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--target" | "-t" if i + 1 < args.len() => {
                target_lang_str = args[i + 1].clone();
                i += 1;
            }
            "--source" | "-s" if i + 1 < args.len() => {
                source_lang_str = args[i + 1].clone();
                i += 1;
            }
            "--output" | "-o" if i + 1 < args.len() => {
                output_path_opt = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let secret_store = StandardSecretStore::new();
    let api_key = secret_store
        .get_api_key()?
        .ok_or("Gemini API key not found. Set GEMINI_API_KEY environment variable or configure via GUI settings.")?;

    let registry = LanguageRegistry::standard();
    let source_id = LanguageId::new(&source_lang_str);
    let target_id = LanguageId::new(&target_lang_str);

    registry
        .validate_pair(&source_id, &target_id)
        .map_err(|e| format!("Language pair validation failed: {}", e))?;

    let audio_engine = Arc::new(FfmpegAudioEngine::new());
    let doc = audio_engine
        .inspect_and_validate(&input_path, 500 * 1024 * 1024)
        .await?;

    let job = audiodub::domain::Job::new(doc, source_id, target_id);
    let job_repo = Arc::new(FileJobRepository::new());

    let settings = AppSettings::default();
    let client = GeminiClient::new(api_key);
    let transcriber = Arc::new(GeminiTranscriber::new(
        client.clone(),
        settings.models.transcriber.clone(),
    ));
    let translator = Arc::new(GeminiTranslator::new(
        client.clone(),
        settings.models.translator.clone(),
    ));
    let synthesizer = Arc::new(GeminiSynthesizer::new(client, settings.models.tts.clone()));

    let orchestrator = PipelineOrchestrator::with_settings(
        transcriber,
        translator,
        synthesizer,
        audio_engine,
        job_repo,
        &settings,
    );

    println!(
        "Starting audio translation: {} -> {}",
        source_lang_str, target_lang_str
    );

    let cancel_token = CancellationToken::new();
    let artifact = orchestrator
        .run_job(job, cancel_token, |j| {
            println!(
                "  [{:>3.0}%] Stage: {:?} — {}",
                j.progress.fraction() * 100.0,
                j.stage,
                j.progress.message
            );
        })
        .await?;

    println!("\n✓ Translation completed successfully!");
    println!("  Output: {}", artifact.path.display());
    println!("  Duration: {}s", artifact.duration_ms / 1000);

    if let Some(dest) = output_path_opt {
        std::fs::copy(&artifact.path, &dest)?;
        println!("  Copied output to: {}", dest.display());
    }

    Ok(())
}
