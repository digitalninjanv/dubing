# AudioDub AI — Native Linux Audio Translation & Voice Dubbing

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![GTK4](https://img.shields.io/badge/GUI-GTK4%20%2B%20Libadwaita-blue.svg)](https://gtk.org)
[![Target](https://img.shields.io/badge/Platform-Fedora%2044%20%7C%20Linux-green.svg)](https://fedoraproject.org)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

**AudioDub AI** adalah aplikasi desktop Linux native modern (GTK4 + Libadwaita + Rust) yang dirancang untuk menerjemahkan dan melakukan *voice dubbing* pada rekaman audio secara otomatis. Aplikasi ini mempertahankan keselarasan waktu percakapan (*timestamp-aware alignment*), mengenali pembicara berbeda (*speaker diarization*), dan merakit hasil secara deterministik di komputer lokal pengguna menggunakan FFmpeg.

---

## Daftar Isi

- [Fitur Utama](#fitur-utama)
- [Arsitektur Hexagonal](#arsitektur-hexagonal)
- [Pipeline Model Gemini](#pipeline-model-gemini)
- [Audio Processing & Alignment](#audio-processing--alignment)
- [Persyaratan Sistem & Dependensi](#persyaratan-sistem--dependensi)
- [Instalasi & Kompilasi](#instalasi--kompilasi)
  - [Membangun Binary Standar](#membangun-binary-standar)
  - [Membangun RPM Fedora 44](#membangun-rpm-fedora-44)
- [Panduan Penggunaan](#panduan-penggunaan)
  - [Penggunaan GUI (Desktop)](#1-penggunaan-gui-desktop)
  - [Penggunaan CLI (Companion)](#2-penggunaan-cli-companion)
- [Keamanan & Privasi](#keamanan--privasi)
- [Pengujian & Verifikasi Kualitas](#pengujian--verifikasi-kualitas)
- [Lisensi](#lisensi)

---

## Fitur Utama

- 🎙️ **Multi-Format Ingestion:** Mendukung MP3, WAV, M4A, AAC, OGG, FLAC, WebM, dan Opus melalui drag-and-drop atau file dialog.
- 🌐 **Dynamic Language Registry (85+ Bahasa):** Deteksi bahasa sumber secara otomatis (`auto`) serta fleksibilitas penerjemahan ke puluhan bahasa target tanpa *hardcoded pairs*.
- 👥 **Multi-Speaker Awareness:** Mempertahankan perbedaan suara untuk dialog 2 pembicara dengan pemetaan profil suara otomatis (Aoede, Kore, Fenrir, Puck).
- ⏱️ **Natural Alignment & Quality Gate:**
  - Penyisipan celah keheningan (*silence padding*) otomatis untuk jeda antar percakapan.
  - Kompresi durasi wajar (*time-stretching*) via filter `atempo` (maksimal 1.25×) guna mencegah distorsi nada (*chipmunk effect*).
  - Peringatan kualitas (*quality warnings*) yang transparan jika audio hasil sintesis melebihi slot target atau beririsan.
- 🛡️ **Crash Recovery & Segment Resume:** Penyimpanan status atomik (`.tmp` → `fsync` → `rename`). Proses yang terputus dapat dilanjutkan (*resumed*) tanpa mengulang sintesis segmen yang telah selesai.
- 🔒 **Local & Private Security Model:**
  - Penyimpanan API Key terenkripsi via Linux Secret Service API (`keyring-rs`) dengan fallback konfigurasi berizin `0600`.
  - Pembersihan otomatis (*auto cleanup*) file segmen temporer setelah ekspor selesai atau saat dibatalkan.
  - Bebas shell injection: semua pemanggilan binary eksternal (`ffmpeg`, `ffprobe`, `xdg-open`) menggunakan structured process command arguments.

---

## Arsitektur Hexagonal

Kode sumber AudioDub AI dibangun dengan arsitektur heksagonal (Ports & Adapters) yang memisahkan logika bisnis dari sistem operasi dan library eksternal:

```text
src/
├── domain/                  # Pure Domain Layer (bebas I/O, GTK, Tokio, FFmpeg)
│   ├── audio.rs             # AudioDocument, AudioFormat, MediaMetadata
│   ├── language.rs          # LanguageId, LanguageRegistry, LanguageInfo
│   ├── transcript.rs        # Transcript, TranscriptSegment, WordTimestamp
│   ├── translation.rs       # TranslatedDocument, TranslationSegment
│   ├── synthesis.rs         # VoiceProfile, SynthesizedSegment, AudioArtifact, AlignmentResult
│   ├── job.rs               # Job, JobId, JobProgress, PipelineStage (12-state machine)
│   └── errors.rs            # DomainError (Permanent vs Transient vs Cancelled)
│
├── application/             # Use Cases & Ports (Interfaces)
│   ├── ports/               # Trait contracts
│   │   ├── audio_engine.rs  # AudioEngine (probe, inspect, align, export)
│   │   ├── transcriber.rs   # SpeechTranscriber (STT)
│   │   ├── translator.rs    # TextTranslator (Translation)
│   │   ├── synthesizer.rs   # SpeechSynthesizer (TTS)
│   │   ├── job_repository.rs# JobRepository (atomic persistence)
│   │   └── secret_store.rs  # SecretStore (keyring & credentials)
│   └── pipeline.rs          # PipelineOrchestrator
│
├── infrastructure/          # Concrete Adapters
│   ├── gemini/              # Gemini HTTP REST (Backoff Retry, Files API, Interactions API)
│   ├── ffmpeg/              # FFmpeg CLI wrapper (ffprobe probe, silence gen, atempo, concat demuxer)
│   ├── filesystem/          # XDG Paths (~/.local/share, ~/.config), atomic JSON manifest, cleanup
│   ├── secrets/             # Keyring & 0600 credentials file
│   └── logging/             # Tracing init & API key redactor
│
├── config/                  # Configuration & Settings
│   └── settings.rs          # AppSettings, ModelsConfig, AudioConfig
│
└── ui/                      # Native GTK4 + Libadwaita Interface
    ├── app.rs               # AdwApplication lifecycle
    ├── window.rs            # MainWindow & async UI channel dispatcher
    ├── views/               # DropzoneView, ProgressView, ResultView, HistoryView, SettingsDialog
    └── components/          # LanguagePickerHelper
```

---

## Pipeline Model Gemini

Sistem memanfaatkan trio model Google Gemini terbaru sesuai spesifikasi resmi:

| Task | Model | Endpoint / Protocol | Format & Karakteristik |
|---|---|---|---|
| **Speech-to-Text** | `gemini-3.5-transcribe` | `POST /v1beta/interactions` & `/upload/v1beta/files` | Ekstraksi transkrip, deteksi bahasa sumber, speaker diarization, dan word timestamps. |
| **Translation** | `gemini-3.1-flash-lite` | `POST /v1beta/models/gemini-3.1-flash-lite:generateContent` | Terjemahan terstruktur dalam mode JSON (`application/json`) dengan pemrosesan chunk per 20 segmen. |
| **Speech Synthesis** | `gemini-3.1-flash-tts-preview` | `POST /v1beta/models/gemini-3.1-flash-tts-preview:generateContent` | Sintesis suara per utterance (chunk < 160 detik, concurrency = 2) menghasilkan audio mono 24 kHz. |

---

## Audio Processing & Alignment

Audio hasil terjemahan diselaraskan kembali ke timeline audio asli melalui proses berikut:

1. **Pre-inspection (`ffprobe`):** Memverifikasi integritas audio input, format, codec, channel, dan durasi (> 0 ms).
2. **Silence Gap Generation:** Jika terdapat jeda antar percakapan pada timeline asli, sistem menghasilkan segmen keheningan alami (`anullsrc`) dengan sample rate yang sesuai.
3. **Adaptive Time-Stretching:**
   - Jika durasi suara terjemahan melebihi slot waktu asli, filter `atempo` diterapkan.
   - Kecepatan dibatasi maksimum **1.25×** untuk menjaga keaslian dan kewajaran suara manusia.
   - Jika durasi tetap melebihi slot waktu pada batas 1.25×, rasio di-*clamp* dan *Quality Warning* dicatat pada laporan akhir.
4. **Assembly & MP3 Export:** Penggabungan segmen terisolasi via FFmpeg concat demuxer dan kompresi ke format MP3 (default 192 kbps, stereo/mono) atau WAV.
5. **Quality Gate Validation:** Memvalidasi output final dengan `ffprobe` sebelum menandai status `Completed`.

---

## Persyaratan Sistem & Dependensi

### Fedora 44+
```bash
sudo dnf install -y gcc rust cargo gtk4-devel libadwaita-devel ffmpeg openssl-devel
```

### Ubuntu 24.04+
```bash
sudo apt update && sudo apt install -y cargo rustc ffmpeg libgtk-4-dev libadwaita-1-dev pkg-config libssl-dev
```

---

## Instalasi & Kompilasi

### Membangun Binary Standar

```bash
# Clone repository
git clone https://github.com/digitalninjanv/audiodub.git
cd audiodub

# Kompilasi rilis berkinerja tinggi
cargo build --release --all-features

# Jalankan binary
./target/release/audiodub --help
```

### Membangun RPM Fedora 44

File spesifikasi RPM lengkap tersedia di `data/packaging/rpm/audiodub.spec`:

```bash
# Siapkan pohon rpmbuild
rpmdev-setuptree

# Buat tarball arsip
tar --exclude-vcs -czf ~/rpmbuild/SOURCES/audiodub-0.1.0.tar.gz .

# Jalankan build RPM
rpmbuild -ba data/packaging/rpm/audiodub.spec

# Instal package yang dihasilkan
sudo dnf localinstall ~/rpmbuild/RPMS/x86_64/audiodub-0.1.0-1.fc44.x86_64.rpm
```

---

## Panduan Penggunaan

### 1. Penggunaan GUI (Desktop)

Jalankan aplikasi desktop dengan perintah:
```bash
audiodub
# atau via menu aplikasi desktop: "AudioDub AI"
```

1. **Pengaturan API Key:** Klik tombol ikon *Preferences* (ikon gir) di sudut kanan atas header bar, masukkan Gemini API Key dari [Google AI Studio](https://aistudio.google.com/), lalu simpan.
2. **Pilih File Audio:** Seret (*drag & drop*) file audio ke area dropzone, atau klik tombol **Choose a File…**.
3. **Pilih Bahasa:**
   - **Source Language:** Biarkan `Auto Detect` atau pilih bahasa spesifik audio sumber.
   - **Target Language:** Pilih bahasa tujuan dubbing (misalnya English, Indonesian, Japanese, German, dll.).
4. **Mulai Proses:** Klik tombol **Translate & Dub Audio**.
5. **Monitor Kemajuan:** Progress bar interaktif akan menampilkan tahapan aktif (`Validating` → `Uploading` → `Transcribing` → `Translating` → `Synthesizing (X of Y)` → `Aligning` → `Exporting`).
6. **Hasil & Pemutaran:** Setelah selesai, jendela hasil menyajikan durasi dan ukuran file, tombol **Play Audio**, tombol **Open Containing Folder**, serta peringatan kualitas bila ada penyesuaian tempo audio.
7. **Riwayat:** Klik ikon riwayat di HeaderBar untuk melihat daftar riwayat pekerjaan dubbing sebelumnya.

### 2. Penggunaan CLI (Companion)

AudioDub AI menyediakan companion CLI lengkap untuk automasi terminal dan skrip:

```bash
# Set Gemini API Key
export GEMINI_API_KEY="AIzaSyYourGeminiApiKeyHere"

# Menerjemahkan audio podcast bahasa Indonesia ke bahasa Inggris
audiodub translate podcast_id.mp3 --source id --target en --output podcast_en.mp3

# Menerjemahkan dengan deteksi bahasa otomatis ke bahasa Jepang
audiodub translate interview.wav --target ja --output interview_ja.mp3
```

---

## Keamanan & Privasi

- **Penyimpanan Kredensial:** Kunci API disimpan pada sistem *FreeDesktop Secret Service* (GNOME Keyring / KWallet) atau pada fallback lokal `~/.config/audiodub/credentials` yang diamankan dengan atribut izin berkas ketat `0600` (hanya dapat dibaca/ditulis oleh pemilik).
- **Isolasi Berkas Sementara:** Setiap pekerjaan diproses dalam sandbox direktori terisolasi `~/.local/share/audiodub/jobs/<job_id>/`.
- **Auto Cleanup:** Berkas segmen suara perantara dibersihkan secara otomatis saat proses selesai atau dibatalkan, kecuali pengguna mengaktifkan opsi *Debug Mode* di pengaturan.
- **Sanitasi Log:** Log tracing dikonfigurasi untuk menyaring dan menyamarkan (*redact*) nilai API key sehingga tidak tercatat pada log sistem.

---

## Pengujian & Verifikasi Kualitas

AudioDub AI menerapkan kebijakan *zero warning* pada linter Clippy dan format kode Rust:

```bash
# 1. Pengecekan formatting
cargo fmt --check

# 2. Linter Clippy ketat
cargo clippy --all-targets --all-features -- -D warnings

# 3. Menjalankan seluruh test suite (Unit, Integrasi, Edge Cases, Real Workflow)
cargo test --all-targets --all-features
```

### Cakupan Pengujian:
- ✅ `domain_test`: State machine transisi 12-state, registry 85+ bahasa, validasi pasangan bahasa.
- ✅ `audio_engine_test`: Probe `ffprobe`, pembuatan tone, alignment silence padding, dan ekspor MP3.
- ✅ `pipeline_flow_test`: Alur kerja PipelineOrchestrator end-to-end dan pengujian pembatalan (*cancellation*).
- ✅ `crash_recovery_test`: Pemulihan manifest crash dan pelanjutan segmen tanpa re-sintesis berulang.
- ✅ `failure_cases_test`: Penolakan file 0-byte, format tidak didukung, berkas korup, batas ukuran, simulasi kegagalan 401 dan retry 429/500 via WireMock.
- ✅ `real_workflow_test`: Eksekusi workflow nyata dari MP3 multi-speaker hingga MP3 output valid terverifikasi `ffprobe`.

---

## Lisensi

Proyek ini dirilis di bawah lisensi [MIT License](LICENSE).
