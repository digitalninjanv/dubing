use audiodub::app::AudioDubApp;
use audiodub::application::ports::{AudioEngine, SecretStore, SpeechSynthesizer};
use audiodub::application::{PipelineOptions, PipelineOrchestrator};
use audiodub::config::AppSettings;
use audiodub::domain::{
    BatchItemStatus, BatchJob, DubbingEngine, LanguageId, LanguageRegistry, SpeakerVoiceConfig,
    TranslationTone, VoiceProfile,
};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use audiodub::infrastructure::filesystem::FileJobRepository;
use audiodub::infrastructure::gemini::{
    GeminiClient, GeminiLiveTranslator, GeminiSynthesizer, GeminiTranscriber, GeminiTranslator,
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
    if args.len() > 1
        && (args[1] == "translate"
            || args[1] == "batch"
            || args[1] == "tts"
            || args[1] == "--help"
            || args[1] == "-h")
    {
        if args[1] == "--help" || args[1] == "-h" {
            print_usage();
            return glib::ExitCode::SUCCESS;
        }

        if args[1] == "translate" {
            if let Err(e) = run_translate_cli(&args[2..]).await {
                eprintln!("\nError: {}", e);
                return glib::ExitCode::FAILURE;
            }
            return glib::ExitCode::SUCCESS;
        }

        if args[1] == "batch" {
            if let Err(e) = run_batch_cli(&args[2..]).await {
                eprintln!("\nError: {}", e);
                return glib::ExitCode::FAILURE;
            }
            return glib::ExitCode::SUCCESS;
        }

        if args[1] == "tts" {
            if let Err(e) = run_tts_cli(&args[2..]).await {
                eprintln!("\nError: {}", e);
                return glib::ExitCode::FAILURE;
            }
            return glib::ExitCode::SUCCESS;
        }
    }

    // Default: Launch Native Desktop GUI
    AudioDubApp::run()
}

fn print_usage() {
    println!("AudioDub AI — Spoken Audio & Video Translation and Dubbing");
    println!("\nUsage:");
    println!("  audiodub                                      Launch GTK4 / Libadwaita GUI");
    println!("  audiodub translate <input> --target <lang>    Translate single file in CLI mode");
    println!("  audiodub batch <file1> <file2> --target <lang> Batch process multiple media files");
    println!("  audiodub tts \"<text>\" [options]               Synthesize expressive speech directly via CLI");
    println!("\nTranslation Options:");
    println!("  --target, -t <lang>       Target language code (e.g., en, id, ja, es, ko)");
    println!("  --source, -s <lang>       Source language code (default: auto)");
    println!("  --output, -o <path>       Output file path");
    println!(
        "  --tone <style>            Translation tone: neutral (default), casual, formal, creative"
    );
    println!(
        "  --voice-1 <name>          Voice name for Speaker 1 (e.g., Kore, Puck, Fenrir, Aoede)"
    );
    println!("  --voice-2 <name>          Voice name for Speaker 2");
    println!("  --subtitles               Export .srt, .vtt, and bilingual transcript files");
    println!(
        "  --engine <mode>           Dubbing engine: studio (default) or live (gemini-3.5-live-translate-preview)"
    );
    println!("  --live, --live-translate  Shortcut for fast real-time Live Translate speech-to-speech");
    println!("\nTTS Studio Options:");
    println!(
        "  --voice, -v <name>        TTS voice: Puck, Charon, Kore, Fenrir, Aoede (default: Puck)"
    );
    println!("  --style <instruction>     Speech style / emotion prompt (e.g. \"whisper\", \"dramatic\")");
    println!("  --speed <float>           Speaking speed multiplier: 0.8 - 1.3 (default: 1.0)");
    println!("  --output, -o <path>       Output audio path (default: synthesized_speech.mp3)");
}

async fn run_translate_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err(
            "Input file path required. Usage: audiodub translate <input.mp3> --target <lang>"
                .into(),
        );
    }

    let input_path = PathBuf::from(&args[0]);
    let mut target_lang_str = "en".to_string();
    let mut source_lang_str = "auto".to_string();
    let mut tone = TranslationTone::Neutral;
    let mut voice_1: Option<String> = None;
    let mut voice_2: Option<String> = None;
    let mut output_path_opt: Option<PathBuf> = None;
    let mut engine = DubbingEngine::Studio;
    let mut duck_audio = false;

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
            "--tone" if i + 1 < args.len() => {
                tone = TranslationTone::from_str_loose(&args[i + 1]);
                i += 1;
            }
            "--voice-1" if i + 1 < args.len() => {
                voice_1 = Some(args[i + 1].clone());
                i += 1;
            }
            "--voice-2" if i + 1 < args.len() => {
                voice_2 = Some(args[i + 1].clone());
                i += 1;
            }
            "--output" | "-o" if i + 1 < args.len() => {
                output_path_opt = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--duck" | "--ducking" => {
                duck_audio = true;
            }
            "--engine" if i + 1 < args.len() => {
                let eng_str = args[i + 1].to_lowercase();
                if eng_str == "live" || eng_str == "live-translate" {
                    engine = DubbingEngine::LiveTranslate;
                }
                i += 1;
            }
            "--live" | "--live-translate" => {
                engine = DubbingEngine::LiveTranslate;
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
    let synthesizer = Arc::new(GeminiSynthesizer::new(
        client.clone(),
        settings.models.tts.clone(),
    ));
    let live_translator = Arc::new(GeminiLiveTranslator::new(
        client,
        settings.models.live_translate.clone(),
    ));

    let orchestrator = PipelineOrchestrator::with_settings(
        transcriber,
        translator,
        synthesizer,
        audio_engine,
        job_repo,
        &settings,
    )
    .with_live_translator(live_translator);

    let voice_config = if voice_1.is_some() || voice_2.is_some() {
        Some(SpeakerVoiceConfig::new(voice_1, voice_2))
    } else {
        None
    };

    let pipeline_options = PipelineOptions {
        tone,
        voice_config,
        export_subtitles: true,
        engine,
        duck_audio,
        review_transcript: false,
        review_channel: None,
    };

    println!(
        "Starting media translation: {} -> {} (Engine: {}, Tone: {})",
        source_lang_str,
        target_lang_str,
        engine.as_str(),
        tone.as_str()
    );

    let cancel_token = CancellationToken::new();
    let artifact = orchestrator
        .run_job_with_options(job, pipeline_options, cancel_token, |j| {
            println!(
                "  [{:>3.0}%] Stage: {:?} — {}",
                j.progress.fraction() * 100.0,
                j.stage,
                j.progress.message
            );
        })
        .await?;

    println!("\n✓ Translation completed successfully!");
    println!("  Output Audio: {}", artifact.path.display());
    println!("  Duration: {}s", artifact.duration_ms / 1000);

    if let Some(ref vid) = artifact.video_path {
        println!("  🎬 Dubbed Video: {}", vid.display());
    }
    if let Some(ref srt) = artifact.subtitle_srt_path {
        println!("  📄 Subtitles (SRT): {}", srt.display());
    }
    if let Some(ref vtt) = artifact.subtitle_vtt_path {
        println!("  📄 Subtitles (VTT): {}", vtt.display());
    }
    if let Some(ref txt) = artifact.transcript_txt_path {
        println!("  📝 Bilingual Script: {}", txt.display());
    }

    if let Some(dest) = output_path_opt {
        std::fs::copy(&artifact.path, &dest)?;
        println!("  Copied audio output to: {}", dest.display());
    }

    Ok(())
}

async fn run_batch_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut file_paths: Vec<PathBuf> = Vec::new();
    let mut target_lang_str = "en".to_string();
    let mut source_lang_str = "auto".to_string();
    let mut tone = TranslationTone::Neutral;
    let mut voice_1: Option<String> = None;
    let mut voice_2: Option<String> = None;

    let mut i = 0;
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
            "--tone" if i + 1 < args.len() => {
                tone = TranslationTone::from_str_loose(&args[i + 1]);
                i += 1;
            }
            "--voice-1" if i + 1 < args.len() => {
                voice_1 = Some(args[i + 1].clone());
                i += 1;
            }
            "--voice-2" if i + 1 < args.len() => {
                voice_2 = Some(args[i + 1].clone());
                i += 1;
            }
            arg if !arg.starts_with('-') => {
                file_paths.push(PathBuf::from(arg));
            }
            _ => {}
        }
        i += 1;
    }

    if file_paths.is_empty() {
        return Err("No input files specified for batch translation. Usage: audiodub batch <file1> <file2> --target <lang>".into());
    }

    let mut batch = BatchJob::new(file_paths, source_lang_str.clone(), target_lang_str.clone());
    println!(
        "Queued {} files for batch translation ({} -> {}, Tone: {})",
        batch.total_count(),
        source_lang_str,
        target_lang_str,
        tone.as_str()
    );

    let secret_store = StandardSecretStore::new();
    let api_key = secret_store
        .get_api_key()?
        .ok_or("Gemini API key not found.")?;

    let registry = LanguageRegistry::standard();
    let source_id = LanguageId::new(&source_lang_str);
    let target_id = LanguageId::new(&target_lang_str);
    registry.validate_pair(&source_id, &target_id)?;

    let audio_engine = Arc::new(FfmpegAudioEngine::new());
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
        audio_engine.clone(),
        job_repo,
        &settings,
    );

    let voice_config = if voice_1.is_some() || voice_2.is_some() {
        Some(SpeakerVoiceConfig::new(voice_1, voice_2))
    } else {
        None
    };

    let total = batch.items.len();
    for (idx, item) in batch.items.iter_mut().enumerate() {
        println!(
            "\n[{}/{}] Processing: {}",
            idx + 1,
            total,
            item.input_path.display()
        );
        item.status = BatchItemStatus::Processing;

        let doc = match audio_engine
            .inspect_and_validate(&item.input_path, settings.audio.max_file_size_bytes)
            .await
        {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  ✗ Validation failed: {}", e);
                item.status = BatchItemStatus::Failed;
                item.error_message = Some(e.to_string());
                continue;
            }
        };

        let job = audiodub::domain::Job::new(doc, source_id.clone(), target_id.clone());
        let cancel_token = CancellationToken::new();

        let pipeline_options = PipelineOptions {
            tone,
            voice_config: voice_config.clone(),
            export_subtitles: true,
            engine: DubbingEngine::default(),
            duck_audio: false,
            review_transcript: false,
            review_channel: None,
        };

        match orchestrator
            .run_job_with_options(job, pipeline_options, cancel_token, |j| {
                println!(
                    "  [{:>3.0}%] Stage: {:?} — {}",
                    j.progress.fraction() * 100.0,
                    j.stage,
                    j.progress.message
                );
            })
            .await
        {
            Ok(artifact) => {
                println!("  ✓ Finished: {}", artifact.path.display());
                if let Some(ref vid) = artifact.video_path {
                    println!("    🎬 Video: {}", vid.display());
                }
                item.output_audio = Some(artifact.path);
                item.output_video = artifact.video_path;
                item.subtitle_path = artifact.subtitle_srt_path;
                item.status = BatchItemStatus::Completed;
            }
            Err(e) => {
                eprintln!("  ✗ Pipeline error: {}", e);
                item.status = BatchItemStatus::Failed;
                item.error_message = Some(e.to_string());
            }
        }
    }

    println!("\n================ Batch Processing Summary ================");
    println!("Total files: {}", batch.total_count());
    println!("Completed:   {}", batch.completed_count());
    println!("Failed:      {}", batch.failed_count());
    for item in &batch.items {
        println!("  {:?} — {}", item.status, item.input_path.display());
        if let Some(ref err) = item.error_message {
            println!("    Reason: {}", err);
        }
    }
    println!("==========================================================\n");

    Ok(())
}

async fn run_tts_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err(
            "Text required. Usage: audiodub tts \"<text>\" [--voice <name>] [--style <prompt>] [--speed <float>] [--output <path>]"
                .into(),
        );
    }

    let text = args[0].clone();
    let mut voice_name = "Puck".to_string();
    let mut style_instruction: Option<String> = None;
    let mut speed: f32 = 1.0;
    let mut output_path = PathBuf::from("synthesized_speech.mp3");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--voice" | "-v" if i + 1 < args.len() => {
                voice_name = args[i + 1].clone();
                i += 1;
            }
            "--style" if i + 1 < args.len() => {
                style_instruction = Some(args[i + 1].clone());
                i += 1;
            }
            "--speed" if i + 1 < args.len() => {
                if let Ok(s) = args[i + 1].parse::<f32>() {
                    speed = s.clamp(0.5, 2.0);
                }
                i += 1;
            }
            "--output" | "-o" if i + 1 < args.len() => {
                output_path = PathBuf::from(&args[i + 1]);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let secret_store = StandardSecretStore::new();
    let api_key = match secret_store.get_api_key() {
        Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
        _ => match env::var("GEMINI_API_KEY") {
            Ok(k) if !k.trim().is_empty() => k.trim().to_string(),
            _ => {
                return Err(
                    "Gemini API key not found. Set GEMINI_API_KEY env or configure in GUI Settings."
                        .into(),
                );
            }
        },
    };

    println!("Starting Text-to-Speech synthesis...");
    println!("  Voice:  {}", voice_name);
    if let Some(ref style) = style_instruction {
        println!("  Style:  {}", style);
    }
    println!("  Speed:  {:.2}x", speed);
    println!("  Output: {}", output_path.display());

    let settings = AppSettings::default();
    let client = GeminiClient::new(api_key);
    let synthesizer = GeminiSynthesizer::new(client, settings.models.tts.clone());

    let voice_profile = VoiceProfile {
        id: voice_name.to_lowercase(),
        voice_name,
        language: "auto".to_string(),
        style: style_instruction.clone(),
        speed,
    };

    let temp_wav = tempfile::Builder::new()
        .prefix("tts_cli_")
        .suffix(".wav")
        .tempfile()?;
    let temp_wav_path = temp_wav.path().to_path_buf();

    let seg = synthesizer
        .synthesize_text(
            &text,
            &voice_profile,
            style_instruction.as_deref(),
            &temp_wav_path,
        )
        .await?;

    let final_wav_path = if (speed - 1.0).abs() > 0.05 {
        let stretched_wav = tempfile::Builder::new()
            .prefix("tts_cli_stretched_")
            .suffix(".wav")
            .tempfile()?;
        let stretched_path = stretched_wav.path().to_path_buf();
        audiodub::infrastructure::ffmpeg::FfmpegAligner::time_stretch(
            &temp_wav_path,
            &stretched_path,
            speed as f64,
        )?;
        stretched_path
    } else {
        temp_wav_path
    };

    if output_path.extension().and_then(|s| s.to_str()) == Some("wav") {
        std::fs::copy(&final_wav_path, &output_path)?;
    } else {
        let status = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&final_wav_path)
            .arg("-c:a")
            .arg("libmp3lame")
            .arg("-b:a")
            .arg("192k")
            .arg(&output_path)
            .status()?;
        if !status.success() {
            return Err("ffmpeg MP3 encoding failed".into());
        }
    }

    println!("\n✓ Text-to-Speech completed successfully!");
    println!("  Saved to: {}", output_path.display());
    println!("  Duration: {:.2}s\n", seg.duration_ms as f64 / 1000.0);

    Ok(())
}
