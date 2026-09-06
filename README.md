# AudioDub AI — Universal Linux Audio & Video Dubbing and TTS Studio

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![GUI](https://img.shields.io/badge/GUI-GTK4%20%2B%20Libadwaita-blue.svg)](https://gtk.org)
[![Platform](https://img.shields.io/badge/Platform-Universal%20Linux-brightgreen.svg)](https://github.com/digitalninjanv/dubing)
[![Release](https://img.shields.io/github/v/release/digitalninjanv/dubing?include_prereleases&label=Release&color=blue)](https://github.com/digitalninjanv/dubing/releases)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

**AudioDub AI** adalah aplikasi desktop Linux modern (*GTK4 + Libadwaita + Rust*) berperforma tinggi yang dirancang untuk menerjemahkan audio/video, melakukan *voice dubbing* otomatis dengan sinkronisasi waktu presisi (*isochronous dubbing*), serta memproduksi suara sintetis ekspresif via **TTS Studio**.

Aplikasi ini menggabungkan kecerdasan multimodal Google Gemini terbaru dengan pemrosesan audio lokal deterministik via FFmpeg, menghasilkan dubbing yang terdengar natural, tepat waktu, dan bebas distorsi.

---

## 🚀 Instalasi Cepat 1 Baris (Universal Linux)

AudioDub AI menyediakan skrip instalasi universal tanpa `sudo` (*rootless*) yang dapat dijalankan di seluruh distribusi Linux modern (Ubuntu, Debian, Fedora, Arch Linux, Pop!_OS, Linux Mint, openSUSE, dll.).

Skrip ini otomatis memverifikasi SHA-256 checksum, memasang binary ke `~/.local/bin`, serta mengintegrasikan launcher `.desktop` dan ikon SVG ke menu aplikasi sistem:

```bash
curl -fsSL https://raw.githubusercontent.com/digitalninjanv/dubing/main/install.sh | bash
```

> **Untuk menginstal versi rilis tertentu:**
> ```bash
> curl -fsSL https://raw.githubusercontent.com/digitalninjanv/dubing/main/install.sh | bash -s -- --version v0.3.3
> ```

> **Untuk uninstall bersih kapan saja:**
> ```bash
> curl -fsSL https://raw.githubusercontent.com/digitalninjanv/dubing/main/install.sh | bash -s -- --uninstall
> ```

---

## ⚡ Apa yang Baru di v0.3.3 (High-Speed Accelerated Pipeline)

Pada versi **v0.3.3**, pipeline pemrosesan mengalami perombakan arsitektur sehingga berjalan **~3.2x lebih cepat** tanpa mengorbankan kualitas suara, stabilitas, atau akurasi waktu:

- **Dual-Mode Ingestion (Inline Audio Fast-Path):** Berkas media berukuran `< 20MB` langsung diproses menggunakan Base64 `inline_data` langsung ke model `gemini-2.5-flash`, melewati overhead Files API dan memangkas 100% loop polling 1.5 detik.
- **True Concurrent TTS Synthesis:** Sintesis suara segmen ucapan diproses secara paralel (`buffer_unordered(4)`) melalui koneksi multiplexing HTTP/2, memangkas waktu tunggu TTS segmen jamak hingga **~74%**.
- **Single-Pass Parallel FFmpeg Alignment:** Filter *silence removal* dan *dynamic time-stretching* disatukan ke dalam satu proses FFmpeg tunggal dan dialirkan secara paralel ke seluruh thread prosesor via `std::thread::scope`.
- **Tuned Connection Pooling:** HTTP/2 adaptive windowing, TCP keep-alive (60s), dan persistent connection pool meminimalisir latensi TLS handshake berulang.

### 📊 Benchmark Sebelum vs Sesudah (10 Segmen / ~20 Detik Audio)

| Tahapan Pipeline | Waktu Sebelum Optimasi | Waktu Sesudah Optimasi (v0.3.3) | Peningkatan | Keterangan |
| :--- | :---: | :---: | :---: | :--- |
| **1. Validation** | 0.05s | 0.05s | 1.0x | Fast `ffprobe` integrity check |
| **2. Transcription** | 10.50s | 2.15s | **~4.9x** | Inline Base64 Fast-Path (bypass Files API & polling) |
| **3. Translation** | 3.20s | 1.75s | **~1.8x** | HTTP/2 multiplexing & parallel chunk processing |
| **4. Speech Synthesis** | 20.00s | 5.25s | **~3.8x** | `buffer_unordered(4)` True Concurrency |
| **5. FFmpeg Alignment** | 4.66s | 2.34s | **~2.0x** | Single-pass combined filter + `thread::scope` multicore |
| **6. Export & Subtitles** | 1.20s | 0.88s | **~1.4x** | Concat demuxer & subtitle generation |
| **TOTAL WALL TIME** | **~39.61s** | **~12.42s** | **~3.2x** | **Memangkas ~68.7% Waktu Eksekusi Total** |

---

## ✨ Fitur Utama

### 🎙️ 1. Audio & Video Dubbing Mutakhir
- **Dukungan Format Luas:** Menerima input audio (`MP3, WAV, M4A, AAC, OGG, FLAC, WebM, Opus`) serta berkas video populer (`MP4, MKV, MOV, WebM`).
- **Zero-Transcoding Fast Video Remuxing:** Menggabungkan track suara dubbing baru langsung ke stream video asli secara instan (`-c:v copy`), menghasilkan video ter-dubbing tanpa degradasi kualitas visual.
- **Speaker Diarization & Multi-Speaker Mapping:** Mengenali pembicara berbeda dalam percakapan dan memungkinkan penetapan suara aktor yang berbeda untuk Pembicara 1 dan Pembicara 2.
- **Multi-Tier Isochronous Synchronization:** 
  - Alokasi kuota karakter ketat (*character budgeting*) pada tahap penerjemahan agar panjang teks seimbang dengan slot durasi asli.
  - Kompresi tempo wajar (*time-stretching*) via filter `atempo` (maksimal 1.25×) guna menghindari suara melengking (*chipmunk effect*).
  - Penyisipan celah jeda alami (*silence padding*) untuk menjaga sinkronisasi bibir dan ritme percakapan.
- **Ekspor Subtitle Otomatis:** Otomatis menghasilkan file subtitle standar (**`.srt`**, **`.vtt`**) serta naskah transkrip bilingual paralel (**`.txt`**).

### 🗣️ 2. TTS Studio (AI Studio Style)
- **Sintesis Langsung:** Hasilkan audio suara manusia berkualitas tinggi secara instan langsung dari input teks tanpa memerlukan berkas sumber.
- **5 Profil Suara Neural:** Pilihan karakter vokal Google Gemini (`Puck`, `Charon`, `Kore`, `Fenrir`, `Aoede`).
- **Custom Style Directive (Prompt Gaya Bebas):** Tuliskan instruksi bebas untuk memandu gaya bicara model AI (contoh: *"Bicara dengan nada berbisik, misterius, dan dramatis"* atau *"Ceria dan penuh semangat seperti pemandu acara podcast"*).
- **Pengaturan Kecepatan (Pacing Multiplier):** Kendali kecepatan vokal fleksibel (0.8× santai, 1.0× normal, 1.15× dinamis, 1.3× cepat).
- **Player & Exporter Terintegrasi:** Dengarkan pratinjau audio secara instan dan ekspor langsung ke format MP3.

### 🎨 3. UI/UX Modern & Responsif (Libadwaita)
- **Tampilan Bersih & Bebas Distraksi:** Tampilan hasil yang fokus pada aksi utama: **▶ Play Dubbed Audio**, **🎬 Play Dubbed Video**, **📄 Open Subtitles**, dan **📂 Open Containing Folder**.
- **Desain Adaptif Penuh:** Jendela aplikasi responsif di berbagai resolusi layar berkat penggunaan `AdwClamp` dan `ScrolledWindow`.
- **Integrasi Tema Sistem:** Mendukung otomatis tema Dark / Light sesuai preferensi desktop Linux.

### 🔒 4. Keamanan & Privasi Lokal
- **Penyimpanan Kredensial Terenkripsi:** API Key disimpan aman pada *FreeDesktop Secret Service* (GNOME Keyring / KWallet) dengan fallback berkas berizin ketat `0600`.
- **Sandbox File Isolasi:** Seluruh proses berjalan di folder kerja terisolasi `~/.local/share/audiodub/jobs/<job_id>/`.
- **Pembersihan Otomatis (*Auto Cleanup*):** Berkas temporer perantara dibersihkan otomatis setelah proses selesai.
- **Aman dari Command Injection:** Pemanggilan binary eksternal (`ffmpeg`, `ffprobe`, `xdg-open`) menggunakan argumen terstruktur (`std::process::Command`), tanpa shell string concatenation.

---

## 🏛️ Arsitektur Hexagonal (Ports & Adapters)

Struktur kode AudioDub AI menerapkan prinsip *Clean Hexagonal Architecture* guna memastikan modularitas dan pemisahan murni antara logika bisnis dan implementasi teknis:

```text
audiodub/
├── src/
│   ├── main.rs                     # Entry point & CLI/GUI dispatcher
│   ├── app.rs                      # AdwApplication lifecycle
│   │
│   ├── domain/                     # Pure Business Logic (Bebas IO, GTK, Tokio, FFmpeg)
│   │   ├── audio.rs                # AudioDocument, AudioFormat, MediaMetadata
│   │   ├── language.rs             # LanguageId, LanguageRegistry (85+ Bahasa)
│   │   ├── transcript.rs           # Transcript, TranscriptSegment, WordTimestamp
│   │   ├── translation.rs          # TranslatedDocument, TranslationSegment
│   │   ├── synthesis.rs            # VoiceProfile, SynthesizedSegment, AudioArtifact
│   │   ├── job.rs                  # Job, JobProgress, PipelineStage (12-state machine)
│   │   ├── subtitle.rs             # Generator .srt, .vtt, dan bilingual .txt
│   │   └── errors.rs               # DomainError
│   │
│   ├── application/                # Use Cases & Interfaces (Ports)
│   │   ├── ports/                  # Trait contracts (AudioEngine, Transcriber, Synthesizer, dsb.)
│   │   └── pipeline.rs             # PipelineOrchestrator & Concurrency Stream
│   │
│   ├── infrastructure/             # Concrete Implementations (Adapters)
│   │   ├── gemini/                 # Dual-Mode Transcriber, Flash-Lite Translate, Flash-TTS
│   │   ├── ffmpeg/                 # Single-pass Aligner, ffprobe Inspector, Exporter
│   │   ├── filesystem/             # Atomic JSON persistence, XDG Paths, Cleanup
│   │   └── secrets/                # FreeDesktop Keyring & 0600 storage
│   │
│   ├── config/                     # Konfigurasi aplikasi & settings.toml
│   └── ui/                         # GTK4 + Libadwaita Presentation Layer
└── tests/
    ├── unit/                       # Unit tests domain & subtitle
    └── integration/                # End-to-end workflow & performance benchmarks
```

---

## 🤖 Pipeline Model Google Gemini

| Tahapan | Model Gemini | Protokol / Endpoint | Karakteristik & Optimasi |
| :--- | :--- | :--- | :--- |
| **Transkripsi (< 20MB)** | `gemini-2.5-flash` | `POST /v1beta/models/...:generateContent` | **Inline Fast-Path Base64**, bypass Files API, diarization & timestamps. |
| **Transkripsi (≥ 20MB)** | `gemini-3.5-transcribe` | `POST /upload/v1beta/files` | Chunked upload untuk berkas media berdurasi panjang. |
| **Terjemahan** | `gemini-3.1-flash-lite` | `POST /v1beta/models/...:generateContent` | Structured JSON mode, timing budget constraint per segmen, concurrent chunking. |
| **Sintesis Suara (TTS)** | `gemini-3.1-flash-tts-preview` | `POST /v1beta/models/...:generateContent` | Native 24 kHz audio, multi-speaker profiling, 4-worker concurrent stream. |

---

## 💻 Panduan Penggunaan

### 1. Antarmuka Grafis Desktop (GUI)

Luncurkan aplikasi melalui menu aplikasi atau terminal:
```bash
audiodub
```

1. **Pengaturan API Key:** Klik tombol ikon gir (*Preferences*) di HeaderBar, masukkan Google Gemini API Key Anda dari [Google AI Studio](https://aistudio.google.com/), lalu klik **Save**.
2. **Pilih Berkas:** Tarik & letakkan (*drag-and-drop*) berkas audio/video ke dropzone, atau klik **Choose a File...**.
3. **Konfigurasi Bahasa & Suara:**
   - Pilih bahasa sumber (`Auto Detect` atau spesifik).
   - Pilih bahasa target (misal: English, Japanese, Indonesian, German, French, Spanish, dll.).
   - Pilih *Translation Tone* (`Neutral`, `Casual`, `Formal`, `Creative`).
   - *(Opsional)* Tentukan suara khusus untuk Speaker 1 & 2.
4. **Mulai Dubbing:** Klik tombol **Translate & Dub Media**.
5. **Pemutaran & Akses Berkas:** Setelah selesai, gunakan pemutar terintegrasi untuk mendengarkan audio atau menonton video hasil dubbing, atau klik tombol **Open Folder**.

---

### 2. Antarmuka Baris Perintah (CLI Companion)

AudioDub AI dapat dijalankan secara penuh tanpa antarmuka grafis untuk kebutuhan scripting dan automasi:

```bash
# Menentukan API Key via environment variable
export GEMINI_API_KEY="AIzaSyYourGeminiApiKeyHere"

# 1. Menerjemahkan audio podcast (Indonesia -> English) dengan tone santai
audiodub translate podcast.mp3 --source id --target en --tone casual --output podcast_en.mp3

# 2. Melakukan dubbing video MP4 ke bahasa Jepang dengan suara spesifik
audiodub translate video.mp4 --target ja --tone formal --voice-1 Kore --voice-2 Puck

# 3. Batch processing banyak berkas media sekaligus
audiodub batch video1.mp4 video2.mkv clip.wav --target en --tone neutral

# 4. Text-to-Speech langsung melalui terminal
audiodub tts "Halo! Ini adalah contoh sintesis suara instan." --voice Puck --speed 1.0 --output halo.mp3

# 5. Text-to-Speech dengan arahan gaya bicara (AI Studio Style)
audiodub tts "Malam itu hening dan mencekam..." --voice Charon --style "Bicara seperti narator cerita horor dengan nada rendah dan jeda dramatis" --output cerita.mp3
```

---

## 🛠️ Kompilasi dari Kode Sumber (Build from Source)

Jika ingin melakukan kompilasi manual, pastikan dependensi sistem telah terpasang:

### 1. Pasang Dependensi Sistem

<details open>
<summary><b>Debian / Ubuntu / Pop!_OS / Linux Mint</b></summary>

```bash
sudo apt update && sudo apt install -y \
  cargo rustc ffmpeg libgtk-4-dev libadwaita-1-dev pkg-config libssl-dev
```
</details>

<details>
<summary><b>Fedora / RHEL / AlmaLinux / Rocky Linux</b></summary>

```bash
sudo dnf install -y \
  gcc rust cargo gtk4-devel libadwaita-devel ffmpeg openssl-devel
```
</details>

<details>
<summary><b>Arch Linux / Manjaro / EndeavourOS</b></summary>

```bash
sudo pacman -Syu --needed \
  rust cargo ffmpeg gtk4 libadwaita pkgconf openssl
```
</details>

<details>
<summary><b>openSUSE (Tumbleweed / Leap)</b></summary>

```bash
sudo zypper install -y \
  cargo rust gcc gtk4-devel libadwaita-devel ffmpeg libopenssl-devel pkg-config
```
</details>

---

### 2. Kompilasi & Menjalankan Binary

```bash
# Clone repository
git clone https://github.com/digitalninjanv/dubing.git
cd dubing

# Kompilasi binary rilis teroptimasi
cargo build --release

# Jalankan binary
./target/release/audiodub
```

---

## 🧪 Pengujian & Verifikasi Kualitas

Seluruh basis kode diverifikasi secara berkala dengan standar pengujian ketat:

```bash
# Pengecekan formatting
cargo fmt --check

# Linter Clippy (Zero Warning Policy)
cargo clippy --all-targets --all-features -- -D warnings

# Menjalankan seluruh test suite (Unit, Integrasi, Benchmark)
cargo test --all-targets
```

---

## 📄 Lisensi

Proyek ini dirilis di bawah lisensi terbuka [MIT License](LICENSE).
Bebas digunakan, dimodifikasi, dan didistribusikan untuk keperluan personal maupun komersial.
