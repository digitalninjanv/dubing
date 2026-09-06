# AGENTS.md — AudioDub AI Implementation Guidelines

Dokumen ini adalah pedoman operasional tunggal bagi AI Coding Agent dan Developer yang bekerja pada repository **AudioDub AI**. Semua kontributor wajib mematuhi panduan ini dan merujuk ke [`AudioDub_AI_PRD (1).md`](./AudioDub_AI_PRD%20(1).md) sebagai Single Source of Truth.

---

## 1. Core Principles & Golden Rules

1. **Strict PRD Adherence:** Jangan menambahkan fitur di luar PRD (no scope creep, no premature video dubbing, no voice cloning, no cloud databases).
2. **Clean Hexagonal Architecture:**
   - `domain/`: Pure business logic, entity, value objects, state machine. Bebas dari dependency IO, GTK, Tokio, Reqwest, atau FFmpeg.
   - `application/`: Use cases & orchestration (`run_job`, `resume_job`, `cancel_job`). Berinteraksi dengan infrastruktur hanya melalui traits (Ports).
   - `infrastructure/`: Implementasi konkret (Adapters) untuk Gemini HTTP REST, FFmpeg CLI wrapper, filesystem storage, dan Secret Service.
   - `ui/`: GTK4 + Libadwaita native desktop interface. Tidak boleh ada blocking IO di thread UI utama.
3. **No Secrets in Code:** Jangan pernah meng-hardcode API key, memasukkan secrets ke git, atau mencatat secrets/audio bytes di log.
4. **Command Safety:** Selalu jalankan binary eksternal (`ffmpeg`, `ffprobe`) melalui structured `std::process::Command` dengan argumen terpisah. **Dilarang keras** menggunakan shell string concatenation (`sh -c "ffmpeg ..."`) untuk mencegah injection.
5. **Incremental Verification:** Setiap perubahan modul harus segera diverifikasi dengan `cargo check`, `cargo test`, dan `cargo clippy --all-targets --all-features -- -D warnings`.

---

## 2. Directory Structure

```text
audiodub/
├── Cargo.toml
├── build.rs                        # Asset embedding & gschema compilation (bila diperlukan)
├── data/
│   ├── io.github.digitalninjanv.AudioDub.desktop
│   ├── io.github.digitalninjanv.AudioDub.metainfo.xml
│   ├── icons/
│   │   └── hicolor/scalable/apps/io.github.digitalninjanv.AudioDub.svg
│   └── packaging/
│       └── rpm/
│           └── audiodub.spec
├── src/
│   ├── main.rs                     # Entry point & CLI/GUI dispatcher
│   ├── app.rs                      # AdwApplication setup & lifecycle
│   │
│   ├── domain/                     # Pure domain layer
│   │   ├── mod.rs
│   │   ├── audio.rs                # AudioDocument, AudioFormat, MediaMetadata
│   │   ├── language.rs             # LanguageId, LanguageRegistry, CapabilityMatrix
│   │   ├── transcript.rs           # Transcript, TranscriptSegment, WordTimestamp
│   │   ├── translation.rs          # TranslatedDocument, TranslationSegment
│   │   ├── synthesis.rs            # VoiceProfile, SynthesizedSegment, AudioArtifact
│   │   ├── job.rs                  # Job, JobId, JobState, PipelineStage, JobManifest
│   │   └── errors.rs               # DomainError, RetryableError, FatalError
│   │
│   ├── application/                # Use cases & interfaces (Ports)
│   │   ├── mod.rs
│   │   ├── ports/
│   │   │   ├── mod.rs
│   │   │   ├── transcriber.rs      # SpeechTranscriber trait
│   │   │   ├── translator.rs       # TextTranslator trait
│   │   │   ├── synthesizer.rs      # SpeechSynthesizer trait
│   │   │   ├── audio_engine.rs     # AudioEngine trait (inspect, slice, mix, export)
│   │   │   ├── job_repository.rs   # JobRepository trait (save, load, list)
│   │   │   └── secret_store.rs     # SecretStore trait (get, set, delete API key)
│   │   ├── pipeline.rs             # PipelineOrchestrator
│   │   ├── create_job.rs
│   │   ├── run_job.rs
│   │   ├── cancel_job.rs
│   │   └── resume_job.rs
│   │
│   ├── infrastructure/             # Concrete implementations (Adapters)
│   │   ├── mod.rs
│   │   ├── gemini/
│   │   │   ├── mod.rs
│   │   │   ├── client.rs           # Base HTTP client with retry & rate limiting
│   │   │   ├── files.rs            # Gemini File API client
│   │   │   ├── transcribe.rs       # Gemini 3.5 Transcribe adapter (Interactions API)
│   │   │   ├── translate.rs        # Gemini 3.1 Flash-Lite structured translation adapter
│   │   │   └── tts.rs              # Gemini 3.1 Flash TTS Preview adapter
│   │   ├── ffmpeg/
│   │   │   ├── mod.rs
│   │   │   ├── probe.rs            # Structured ffprobe inspector
│   │   │   ├── aligner.rs          # Audio alignment, padding, time-stretching
│   │   │   └── exporter.rs         # Final MP3 / WAV encoder
│   │   ├── filesystem/
│   │   │   ├── mod.rs
│   │   │   ├── paths.rs            # XDG base directories (~/.local/share, ~/.config)
│   │   │   ├── job_repository.rs   # Atomic JSON manifest persistence
│   │   │   └── cleanup.rs          # Temp directory management & retention
│   │   ├── secrets/
│   │   │   ├── mod.rs
│   │   │   └── keyring.rs          # Linux Secret Service / Keyring integration
│   │   └── logging/
│   │       ├── mod.rs
│   │       └── redact.rs           # Sensitive data redactor for logs
│   │
│   ├── config/                     # Application configuration
│   │   ├── mod.rs
│   │   ├── settings.rs             # AppSettings, ModelsConfig, AudioConfig
│   │   └── defaults.rs
│   │
│   └── ui/                         # GTK4 + Libadwaita presentation
│       ├── mod.rs
│       ├── window.rs               # MainWindow
│       ├── views/
│       │   ├── mod.rs
│       │   ├── dropzone.rs         # Drag-and-drop & file picker view
│       │   ├── progress.rs         # Multi-stage progress with segment indicator
│       │   ├── result.rs           # Result audio player & export buttons
│       │   ├── history.rs          # Local job history view
│       │   └── settings.rs         # Settings dialog (API key, models, dirs)
│       └── components/
│           ├── mod.rs
│           ├── language_picker.rs  # Source/Target dropdown with capability check
│           ├── voice_picker.rs     # Voice profile selector
│           └── error_banner.rs     # Human-readable error banner with retry
└── tests/
    ├── common/
    │   └── mock_providers.rs       # Deterministic mock implementations of ports
    ├── unit/
    │   ├── language_registry_test.rs
    │   ├── state_machine_test.rs
    │   └── alignment_test.rs
    ├── integration/
    │   ├── pipeline_flow_test.rs
    │   ├── crash_recovery_test.rs
    │   └── ffmpeg_export_test.rs
    └── fixtures/
        └── test_sample.mp3
```

---

## 3. Gemini Model Strategy & API Contracts

### 3.1 Speech-to-Text (`gemini-3.5-transcribe`)
- **API Mode:** Pre-recorded audio batch mode uses **Interactions API** (`POST /v1beta/interactions`) with audio uploaded via **Files API** (`/upload/v1beta/files`).
- **Features Activated:**
  - Automatic language detection (85+ locales).
  - Speaker diarization (labels speaker IDs, optimized for 1–2 speakers in MVP).
  - Word/utterance timestamps (`start_ms`, `end_ms`).
- **Safety Policy:** Fallback to standard Gemini File API + structured schema if Interactions API endpoint is gated in specific regions.

### 3.2 Translation (`gemini-3.1-flash-lite`)
- **Model ID:** `gemini-3.1-flash-lite` (GA). **Dilarang keras** menggunakan `gemini-3.1-flash-lite-preview` (telah shutdown 25 Mei 2026).
- **Endpoint:** `POST /v1beta/models/gemini-3.1-flash-lite:generateContent`
- **Output Mode:** JSON mode (`responseMimeType: "application/json"`) dengan schema ketat untuk menjamin segment ID, speaker ID, dan teks terjemahan tetap sinkron dengan source timeline.

### 3.3 Speech Generation / TTS (`gemini-3.1-flash-tts-preview`)
- **Endpoint:** `POST /v1beta/models/gemini-3.1-flash-tts-preview:generateContent`
- **Configuration:**
  ```json
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {
      "voiceConfig": {
        "prebuiltVoiceConfig": {
          "voiceName": "<VoiceName>"
        }
      }
    }
  }
  ```
- **Chunking Rule:** Maksimal 160 detik per panggilan (rekomendasi PRD: proses per utterance/sentence, bukan seluruh audio sekaligus).
- **Response Handling:** Ekstrak base64 PCM/WAV bytes dari inline response dan tulis ke disk temporer per segment.

---

## 4. Audio Pipeline & Alignment Guidelines

1. **Pre-inspection (`ffprobe`):**
   - Validasi durasi (> 0 ms), codec, sample rate, jumlah channels.
   - Tolak file kosong atau korup sebelum mengunggah ke Gemini API.
2. **Alignment & Timing:**
   - Gunakan `start_ms` dan `end_ms` dari transcript asli.
   - Hitung selisih antara durasi hasil TTS (`tts_duration_ms`) dan durasi slot target (`target_slot_ms`).
   - Jika `tts_duration_ms < target_slot_ms`: sisipkan silence gap secara natural.
   - Jika `tts_duration_ms > target_slot_ms`:
     1. Uji kompresi wajar dengan filter `atempo` (maksimal 1.15x - 1.25x agar suara tetap terdengar alami tanpa distorsi pitch).
     2. Jika tetap melebihi batas toleransi, berikan peringatan kualitas (`Quality warnings`).
3. **Assembly & Export (`ffmpeg`):**
   - Gabungkan segmen audio menggunakan filtergraph / concat demuxer terisolasi.
   - Encode output akhir ke MP3 (default `libmp3lame`, 192 kbps) dan opsional WAV.
   - Validasi output akhir dengan `ffprobe` sebelum menandai job `COMPLETED`.

---

## 5. Security & Privacy Protocols

1. **Secret Storage:**
   - Prioritaskan OS Secret Service (FreeDesktop Secret Service API via `keyring-rs`).
   - Fallback ke file konfigurasi terlindungi (`~/.config/audiodub/config.toml`, permissions `0600`).
   - Jangan pernah menyertakan API key di log `tracing`. Gunakan custom formatter/redactor.
2. **File Sandboxing & Temp Isolation:**
   - Setiap job memiliki working directory terisolasi: `~/.local/share/audiodub/jobs/<job_id>/`.
   - Temporary segment audio dan file perantara wajib dibersihkan secara otomatis saat job selesai (kecuali user mengaktifkan Debug Mode di Settings).
   - Startup reconciliation: Hapus direktori temporary orphan yang ditinggalkan oleh crash sebelumnya jika tidak dapat di-resume.

---

## 6. Resilience & State Machine

Status resmi job:
```text
IDLE → VALIDATING → UPLOADING → TRANSCRIBING → TRANSLATING → SYNTHESIZING → ALIGNING → EXPORTING → VALIDATING_OUTPUT → COMPLETED
```
Terminal / Error states:
- `FAILED_RETRYABLE`: Gagal karena masalah jaringan / rate limit 429 / HTTP 5xx. Menyediakan tombol "Coba Lagi".
- `FAILED_PERMANENT`: Gagal karena file korup / API key invalid / bahasa tidak didukung.
- `CANCELLED`: Dibatalkan oleh pengguna via `CancellationToken`.

**Crash Safety:**
- Manifest job (`job.json`) disimpan secara atomik (`write to .tmp` -> `fsync` -> `rename`).
- Saat aplikasi dibuka kembali, sistem memeriksa job berstatus running/interrupted dan menawarkan opsi "Resume" dari segmen terakhir yang berhasil.

---

## 7. Packaging Fedora 44

- Target build: Native RPM package menggunakan `cargo-generate-rpm` atau `.spec` file standar Fedora.
- System dependencies wajib: `gtk4`, `libadwaita`, `ffmpeg`, `glib2`.
- Desktop integration: File `.desktop` terdaftar di FreeDesktop menu, mendukung drag-and-drop file audio, dan memiliki association MIME type audio standar (`audio/mpeg`, `audio/wav`, dll.).
