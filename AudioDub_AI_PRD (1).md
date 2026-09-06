# AudioDub AI — Product Requirements Document (PRD)

**Dokumen:** PRD — AudioDub AI Desktop
**Versi:** 1.0.0
**Status:** Ready for AI-assisted implementation
**Target utama:** Fedora Linux 44+ / GNOME
**Platform awal:** Linux native x86_64, ARM64 direncanakan
**Bahasa dokumen:** Indonesia
**Tanggal riset:** 6 September 2026
**Jenis produk:** AI-powered desktop audio translation & dubbing

---

## 0. Executive Summary

AudioDub AI adalah aplikasi desktop Linux native yang memungkinkan pengguna mengubah satu file audio menjadi versi bahasa lain secara otomatis.

Contoh inti:

```text
podcast-indonesia.mp3
        ↓
Deteksi bahasa
        ↓
Transkripsi + timestamp + speaker
        ↓
Terjemahan semantik
        ↓
AI voice generation
        ↓
Audio alignment + mixing
        ↓
podcast-english.mp3
```

Produk **tidak dibatasi Indonesia → Inggris**. Sistem harus dirancang sebagai **multi-language translation engine**, dengan daftar bahasa yang berasal dari kemampuan model/runtime dan dapat diperbarui tanpa mengubah arsitektur inti.

Contoh pasangan bahasa:

- Indonesia → English
- English → Indonesian
- Japanese → English
- Korean → Indonesian
- Spanish → English
- English → Japanese
- Indonesian → Javanese
- dan pasangan lain yang didukung pipeline AI.

Versi MVP memprioritaskan **file audio → audio hasil terjemahan**, bukan real-time conversation.

---

# 1. Product Vision

## 1.1 Vision

> Make language translation for spoken audio as simple as converting a file: drop audio, choose the target language, and receive a natural translated voice track.

## 1.2 Product Promise

Pengguna tidak perlu memahami transcription, subtitle, TTS, codec, timestamp, FFmpeg, atau API AI. Pengguna cukup:

1. Memilih file.
2. Memilih bahasa tujuan atau Auto.
3. Menekan Translate/Dub.
4. Menunggu pipeline selesai.
5. Mendengarkan dan menyimpan output.

## 1.3 Prinsip Produk

### Simple
Satu layar harus cukup untuk menyelesaikan pekerjaan utama.

### Native
Aplikasi harus terasa seperti aplikasi Linux/GNOME, bukan website yang dibungkus.

### AI-first
AI digunakan pada bagian yang membutuhkan intelligence; pekerjaan deterministik seperti encoding dan file processing tetap lokal.

### Correct by default
Sistem harus memvalidasi hasil transkripsi, translation, timestamp, dan audio sebelum menyatakan job berhasil.

### Resilient
Kegagalan satu segment tidak boleh otomatis menghancurkan seluruh job; sistem harus mendukung retry dan resume.

### Privacy-conscious
Audio adalah data pengguna. Aplikasi harus meminimalkan penyimpanan sementara dan menjelaskan bahwa audio dikirim ke provider AI saat cloud processing digunakan.

### Provider-agnostic architecture
Gemini menjadi provider AI pertama, tetapi core domain tidak boleh dikunci keras ke satu model.

---

# 2. Problem Statement

Menerjemahkan video/audio secara manual membutuhkan banyak langkah:

```text
audio
→ transcription
→ cleanup
→ translation
→ voice recording / TTS
→ timing
→ audio editing
→ export
```

Untuk pengguna biasa, proses tersebut terlalu teknis.

AudioDub AI menggabungkan workflow menjadi satu pipeline desktop:

```text
Input Audio
→ Understand
→ Translate
→ Synthesize
→ Align
→ Export
```

---

# 3. Target Users

## Primary

### A. Content creator
Membuat versi bahasa lain dari podcast, tutorial, interview, audiobook, atau konten edukasi.

### B. Student/researcher
Mendengarkan materi asing dalam bahasa pilihan.

### C. Developer / technical user
Menguji AI speech pipeline dengan aplikasi Linux native.

### D. Multilingual individual
Mengonversi audio personal untuk pemahaman lintas bahasa.

## Secondary

- Podcaster
- Educator
- Translator
- Accessibility users
- Localization teams
- Indie media producers
- Open-source community

---

# 4. Non-Goals MVP

MVP **tidak** wajib mencakup:

- Real-time voice conversation.
- Video dubbing penuh.
- Lip-sync video.
- Voice cloning.
- Training custom voice model.
- Cloud account system.
- Multi-user collaboration.
- Billing/subscription.
- Enterprise admin console.
- Large-scale cloud job orchestration.

Arsitektur harus memungkinkan fitur tersebut di masa depan tanpa merombak domain model.

---

# 5. Key User Stories

## US-001 — Basic translation

> Sebagai pengguna, saya ingin memasukkan file MP3 dan memilih bahasa tujuan agar saya mendapatkan audio hasil terjemahan.

**Acceptance criteria:**

- MP3 dapat dipilih lewat file chooser.
- MP3 dapat di-drag-and-drop.
- Bahasa sumber dapat Auto Detect.
- Bahasa target wajib dipilih.
- Tombol Translate aktif hanya jika input valid.
- Output dapat diputar dan disimpan.

## US-002 — Multi-language

> Sebagai pengguna, saya ingin menerjemahkan audio ke banyak bahasa tanpa aplikasi memiliki workflow terpisah untuk setiap bahasa.

**Acceptance criteria:**

- Source dan target memakai language registry.
- Bahasa tidak di-hardcode di pipeline.
- Unsupported language menghasilkan pesan yang jelas.

## US-003 — Speaker awareness

> Sebagai pengguna, saya ingin dialog dua orang tetap terdengar sebagai dua pembicara berbeda.

**Acceptance criteria:**

- Sistem menyimpan speaker ID dari transcription.
- TTS dapat dipetakan ke voice profile.
- MVP mendukung minimal single-speaker dan dua-speaker output.

## US-004 — Resume failed job

> Sebagai pengguna, saya tidak ingin mengulang semuanya jika satu bagian gagal.

**Acceptance criteria:**

- Job memiliki state per stage.
- Segment yang sukses tidak diproses ulang kecuali diminta.
- Retry hanya pada step gagal.

## US-005 — Local audio processing

> Sebagai pengguna, saya ingin output final diproses secara lokal setelah AI generation sehingga aplikasi tidak membutuhkan server tambahan untuk encoding.

**Acceptance criteria:**

- FFmpeg dijalankan lokal.
- Final MP3 dibuat lokal.
- Temporary files dibersihkan sesuai retention policy.

---

# 6. Core Product Workflow

```text
┌──────────────────────────┐
│ 1. Select / Drop Audio   │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 2. Validate & Inspect    │
│ format / size / duration │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 3. Upload to Gemini      │
│ File API where suitable  │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 4. Transcription         │
│ text + timestamps +      │
│ language + speakers      │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 5. Translation           │
│ source → target          │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 6. TTS / Voice synthesis │
│ segment-by-segment       │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 7. Timing + audio mix    │
│ FFmpeg/local DSP         │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 8. Validate final output │
└────────────┬─────────────┘
             ↓
┌──────────────────────────┐
│ 9. Play / Save / Export  │
└──────────────────────────┘
```

---

# 7. Technology Decision

## 7.1 Native UI

**Primary stack:**

- Rust
- GTK4
- Libadwaita
- gtk4-rs

Alasan:

- Native Linux/GNOME experience.
- Cocok dengan Fedora/GNOME.
- File dialog dan drag-and-drop tersedia di GTK4.
- Libadwaita menyediakan pola UI modern dan adaptive layout.
- Rust memberikan memory safety dan concurrency yang baik.

GTK4 menyediakan API drag-and-drop dan asynchronous file dialog; Libadwaita menyediakan komponen adaptive untuk aplikasi GNOME modern.  

## 7.2 Async runtime

**Tokio**

Dipakai untuk:

- API requests.
- Retry.
- concurrency terbatas.
- job orchestration.
- cancellation.
- background tasks.

## 7.3 HTTP / Gemini client

Gunakan **Google GenAI SDK resmi** yang mendukung Rust jika sudah tersedia/stabil untuk kebutuhan proyek saat implementasi; jika belum memenuhi kebutuhan, gunakan HTTP client Rust terisolasi pada adapter Gemini.

**Penting:** domain/application layer tidak boleh langsung bergantung pada struktur request Gemini.

Recommended abstraction:

```rust
trait SpeechTranscriber {
    async fn transcribe(&self, input: AudioInput) -> Result<Transcript>;
}

trait Translator {
    async fn translate(&self, input: TranslationInput) -> Result<TranslatedDocument>;
}

trait SpeechSynthesizer {
    async fn synthesize(&self, input: SpeechInput) -> Result<AudioArtifact>;
}
```

## 7.4 Audio engine

**FFmpeg CLI** sebagai engine deterministik lokal.

Dipakai untuk:

- inspect media.
- normalize.
- resample.
- trim.
- pad silence.
- concatenate.
- mix.
- encode MP3/WAV/FLAC.
- metadata.

Gunakan wrapper Rust yang menjalankan binary FFmpeg dengan argumen terstruktur. Hindari shell string concatenation untuk mencegah command injection.

## 7.5 Storage

MVP tidak memerlukan database server.

Gunakan local app directories:

```text
~/.local/share/audiodub/
├── projects/
├── jobs/
├── cache/
├── outputs/
└── logs/

~/.config/audiodub/
└── config.toml
```

API key harus disimpan menggunakan mekanisme secure secret storage OS bila memungkinkan; jangan memasukkannya ke source code atau project file.

---

# 8. AI Architecture

## 8.1 Provider

Provider pertama: **Google Gemini API**.

Arsitektur internal harus menggunakan adapter:

```text
Application
   │
   ▼
AI Provider Interface
   │
   ├── Gemini Transcription Adapter
   ├── Gemini Translation Adapter
   └── Gemini TTS Adapter
```

Tujuan:

- model dapat diganti tanpa merombak UI.
- fallback provider dapat ditambahkan.
- benchmark model dapat dilakukan.
- testing bisa memakai mock provider.

---

# 9. Recommended Gemini Model Strategy

## 9.1 Speech-to-Text

**Primary:** `gemini-3.5-transcribe`

Google saat ini mendokumentasikan model ini khusus untuk speech-to-text, dengan automatic language identification, speaker diarization, word-level timestamps, smart transcription, dan custom vocabulary. Model mendeteksi 85+ language locales dan menangani code-switching. citehttps://ai.google.dev/gemini-api/docs/transcribe

**MVP configuration:**

```text
mode: verbatim
language_codes: auto unless user overrides
speaker diarization: enabled when required
word timestamps: enabled when alignment mode requires it
custom vocabulary: optional
```

Catatan penting:

- Word-level timestamps dapat memengaruhi akurasi.
- Custom vocabulary memiliki interaksi/ketidakcocokan dengan fitur tertentu seperti diarization/timestamps.
- Attribution lebih dari 3 speaker bersifat eksperimental menurut dokumentasi model.

Pipeline harus menghormati kombinasi fitur yang benar-benar kompatibel.

## 9.2 Translation

**Default economical translator:** `gemini-3.1-flash-lite`

Google mendeskripsikan model ini sebagai model GA yang dioptimalkan untuk speed, scale/cost efficiency, termasuk translation dan simple data processing. `gemini-3.1-flash-lite` adalah pengganti dari preview model yang shutdown pada 25 Mei 2026. citehttps://ai.google.dev/gemini-api/docs/changelog

Model ID preview berikut **jangan digunakan**:

```text
gemini-3.1-flash-lite-preview
```

Karena sudah shutdown.

## 9.3 TTS

**Primary:** `gemini-3.1-flash-tts-preview`

Google mendokumentasikan TTS ini dengan single-speaker dan multi-speaker support, natural speech, control terhadap style/accent/pace/tone, dan streaming pada versi 3.1. TTS juga mendukung banyak bahasa termasuk Indonesian, English, Japanese, Korean, Spanish, Javanese, Malay, Vietnamese, Chinese Mandarin, dan lainnya. citehttps://ai.google.dev/gemini-api/docs/speech-generation

TTS adalah area yang harus dibuat **capability-driven** karena model preview dapat berubah.

Jangan hardcode assumption seperti “semua source language pasti tersedia sebagai target voice”. Language registry harus menyimpan capability per model.

## 9.4 Live API bukan jalur utama MVP

Gemini Live/Live Translate cocok untuk real-time/audio-to-audio interaction, bukan batch file dubbing utama.

MVP menggunakan:

```text
Transcribe
→ Translate
→ TTS
→ FFmpeg
```

Live pipeline dapat menjadi future product mode.

---

# 10. Multi-Language Design

## 10.1 Language Registry

Buat registry terpusat:

```text
LanguageCode
DisplayName
NativeName
Locale
TranscriptionSupported
TranslationSupported
TTSSupported
DefaultVoice
Direction
```

Contoh:

```json
{
  "code": "id",
  "locale": "id-ID",
  "name": "Indonesian",
  "native_name": "Bahasa Indonesia",
  "transcription": true,
  "translation": true,
  "tts": true
}
```

## 10.2 Jangan mengunci pasangan bahasa

Jangan membuat kode seperti:

```rust
if source == "id" && target == "en" { ... }
```

Gunakan:

```text
source_language: LanguageId
 target_language: LanguageId
```

dan validasi capability matrix.

## 10.3 Auto detection

Default:

```text
source = AUTO
```

User dapat override jika sistem salah mendeteksi.

---

# 11. Translation Quality Strategy

Terjemahan tidak boleh sekadar literal.

Prompt policy:

```text
Translate the transcript into <TARGET_LANGUAGE>.

Requirements:
- Preserve the original meaning.
- Preserve speaker intent.
- Preserve names and technical terminology.
- Do not invent facts.
- Avoid literal phrasing when unnatural.
- Maintain conversational tone.
- Preserve numbers, dates and units correctly.
- Do not translate proper names unless appropriate in the target language.
- Return machine-readable segments only.
```

Untuk konten panjang, translation dilakukan secara segment/chunk dengan konteks minimal yang diperlukan.

Target harus mempertahankan:

```text
segment_id
speaker_id
source_start
source_end
source_text
translated_text
```

---

# 12. TTS Strategy

## 12.1 Segment-based generation

Jangan mengirim transcript beberapa menit sebagai satu TTS request.

Google merekomendasikan pemecahan transcript panjang menjadi bagian lebih kecil karena kualitas/consistency dapat drift pada output panjang. citehttps://ai.google.dev/gemini-api/docs/speech-generation

Recommended:

```text
sentence / utterance
        ↓
TTS
        ↓
segment audio
```

## 12.2 Voice profiles

```text
VoiceProfile {
  id
  provider
  voice_name
  language
  gender_label_optional
  style
  default_speed
}
```

Jangan menyimpulkan identitas manusia asli dari label suara.

## 12.3 Multi-speaker

MVP target:

- Single speaker.
- Two-speaker conversation.

TTS Gemini saat ini mendukung multi-speaker sampai dua speaker pada capability yang didokumentasikan. citehttps://ai.google.dev/gemini-api/docs/speech-generation

Untuk lebih dari dua speaker, pipeline dapat menghasilkan satu TTS call per speaker/segment atau fallback ke voice mapping individual.

---

# 13. Timing & Audio Alignment

Ini adalah bagian pembeda utama dibanding translator teks biasa.

## 13.1 Source timeline

Setiap utterance memiliki:

```json
{
  "segment_id": "seg_0001",
  "speaker_id": "spk_01",
  "start_ms": 0,
  "end_ms": 4250,
  "source_text": "..."
}
```

## 13.2 Target audio timeline

Setelah TTS:

```json
{
  "segment_id": "seg_0001",
  "tts_duration_ms": 3820
}
```

## 13.3 Alignment strategy

Urutan prioritas:

1. Natural target speech.
2. Preserve semantic content.
3. Fit within original segment.
4. Minimize unnatural speed changes.

Jika audio terlalu panjang:

```text
1. revise translation for concision
2. regenerate TTS
3. moderate time-stretch
4. add overlap only when safe
```

Hindari extreme time compression.

## 13.4 Deterministic fallback

Jika timing tidak dapat disejajarkan secara natural:

- sisakan silence/flexible gaps.
- gunakan audio length target.
- tandai quality warning.

MVP tidak wajib melakukan professional lip-sync.

---

# 14. Optional Background Audio / Original Voice Policy

MVP default:

```text
Output = translated voice only
```

Future:

```text
Original audio
  ├── voice
  ├── music
  └── ambience
```

memerlukan source separation.

**Jangan mengklaim voice preservation / source separation pada MVP** jika belum ada model dan evaluasi khusus.

---

# 15. Audio Input Requirements

Minimum accepted formats:

- MP3
- WAV
- M4A
- AAC
- OGG
- FLAC
- WebM audio
- Opus

Gemini 3.5 Transcribe mendokumentasikan dukungan untuk format audio termasuk WAV, MP3, AIFF, AAC, OGG, FLAC, MPEG, M4A, L16, Opus, ALAW, MULAW, dan WebM. citehttps://ai.google.dev/gemini-api/docs/transcribe

App harus tetap melakukan local validation sebelum upload.

---

# 16. File Size / Upload Strategy

Untuk file kecil, inline upload dapat dipertimbangkan.

Untuk file lebih besar atau dipakai beberapa request, gunakan Gemini File API.

Google saat ini mendokumentasikan:

- File API max 2 GB per file.
- Sampai 20 GB per project.
- Standard uploaded files disimpan sementara selama 48 jam.
- Inline payload memiliki batas 100 MB per request. citehttps://ai.google.dev/gemini-api/docs/file-input-methods

Namun aplikasi harus memiliki batas UI sendiri yang lebih konservatif agar UX dan resource lokal tetap sehat.

Recommended MVP policy:

```text
Default max input: 500 MB
Hard configurable max: 2 GB
```

Batas tersebut harus dapat diubah melalui config tanpa memodifikasi domain logic.

---

# 17. UI/UX Requirements

## 17.1 Main Window

Target GNOME-style:

```text
┌────────────────────────────────────────────┐
│ AudioDub AI                            ⋮   │
├────────────────────────────────────────────┤
│                                            │
│             Translate your audio           │
│                                            │
│    ┌──────────────────────────────────┐    │
│    │                                  │    │
│    │     Drop audio file here         │    │
│    │     or choose a file             │    │
│    │                                  │    │
│    └──────────────────────────────────┘    │
│                                            │
│ Source                                     │
│ [ Auto Detect                         ▼ ]  │
│                                            │
│ Translate to                               │
│ [ English                             ▼ ]  │
│                                            │
│ Voice                                      │
│ [ Natural / Default                    ▼ ] │
│                                            │
│        [ Translate & Generate ]             │
│                                            │
└────────────────────────────────────────────┘
```

## 17.2 Progress Screen

Jangan hanya tampilkan “Loading”.

Gunakan stage progress:

```text
✓ Audio validated
✓ Uploading
✓ Transcribing
✓ Translating
● Generating voice
○ Aligning audio
○ Exporting MP3
```

Tampilkan:

- elapsed time.
- estimated progress jika dapat dihitung.
- current segment.
- cancel button.

## 17.3 Result Screen

```text
Original: Indonesian
Target: English
Duration: 03:42
Speakers: 2

[▶ Play result]

[ Save MP3 ]
[ Save WAV ]

Quality warnings: 0
```

## 17.4 History

MVP ringan dapat menyimpan local job history:

```text
Today

Indonesia → English
podcast.mp3
Completed · 03:42

English → Japanese
lecture.m4a
Completed · 12:08
```

---

# 18. UX States

Setiap job memiliki state machine:

```text
IDLE
 ↓
VALIDATING
 ↓
UPLOADING
 ↓
TRANSCRIBING
 ↓
TRANSLATING
 ↓
SYNTHESIZING
 ↓
ALIGNING
 ↓
EXPORTING
 ↓
VALIDATING_OUTPUT
 ↓
COMPLETED
```

Failure states:

```text
FAILED_RETRYABLE
FAILED_PERMANENT
CANCELLED
```

Tidak boleh ada state “unknown” untuk job yang disimpan.

---

# 19. Job Model

```json
{
  "job_id": "job_20260906_001",
  "status": "SYNTHESIZING",
  "source": {
    "file_name": "podcast.mp3",
    "language": "id",
    "duration_ms": 222000
  },
  "target_language": "en",
  "pipeline": {
    "transcription": "completed",
    "translation": "completed",
    "tts": "running",
    "alignment": "pending",
    "export": "pending"
  },
  "segments_total": 58,
  "segments_completed": 39,
  "created_at": "...",
  "updated_at": "..."
}
```

---

# 20. Domain Data Structures

Minimal Rust domain objects:

```rust
struct AudioDocument {
    id: String,
    path: PathBuf,
    mime_type: String,
    duration_ms: u64,
}

struct Transcript {
    language: LanguageId,
    segments: Vec<TranscriptSegment>,
}

struct TranscriptSegment {
    id: String,
    speaker_id: Option<String>,
    start_ms: u64,
    end_ms: u64,
    text: String,
    words: Vec<WordTimestamp>,
}

struct TranslationSegment {
    segment_id: String,
    source_text: String,
    translated_text: String,
}

struct SynthesizedSegment {
    segment_id: String,
    path: PathBuf,
    duration_ms: u64,
}
```

---

# 21. Gemini Adapter Contract

Recommended boundary:

```text
src/
├── domain/
├── application/
│   ├── transcribe.rs
│   ├── translate.rs
│   ├── synthesize.rs
│   └── pipeline.rs
├── infrastructure/
│   ├── gemini/
│   ├── ffmpeg/
│   └── filesystem/
└── ui/
```

Domain tidak boleh import SDK Gemini.

---

# 22. Pipeline Orchestrator

Pseudo-flow:

```rust
async fn run_job(job: Job) -> Result<OutputArtifact> {
    validate_input(&job).await?;

    let remote_file = uploader.upload_if_needed(&job.input).await?;

    let transcript = transcriber.transcribe(
        remote_file,
        job.transcription_options(),
    ).await?;

    let translated = translator.translate(
        transcript,
        job.translation_options(),
    ).await?;

    let segments = synthesizer.generate_all(
        translated,
        job.voice_options(),
    ).await?;

    let aligned = aligner.align(
        job.source_timeline(),
        segments,
    ).await?;

    let output = audio_exporter.export_mp3(aligned).await?;

    validate_output(&output).await?;

    Ok(output)
}
```

---

# 23. Concurrency Strategy

Jangan melakukan unlimited parallel requests.

Recommended MVP:

```text
transcription: 1 request
translation: controlled chunk concurrency = 2–4
TTS: controlled concurrency = 2
FFmpeg: 1 final mixing job
```

Concurrency harus configurable.

Tujuan:

- menghindari API rate limit.
- menghindari saturasi CPU/RAM.
- menjaga UX desktop.
- mengurangi cost spike.

---

# 24. Retry Strategy

Retry hanya untuk error transient.

Contoh retryable:

- network timeout.
- connection reset.
- 429 / rate limit.
- 5xx.
- temporary Gemini processing failure.

Backoff:

```text
1s
2s
4s
8s
max 30s
max attempts 4
```

Gunakan jitter.

Non-retryable:

- unsupported format.
- invalid API key.
- unsupported language.
- malformed request.
- safety/content rejection yang konsisten.

Untuk Gemini 3.1 Flash TTS Preview, dokumentasi menyebut occasional text-token returns dapat menyebabkan server error, sehingga aplikasi memang perlu automated retry. citehttps://ai.google.dev/gemini-api/docs/speech-generation

---

# 25. Cancellation

Cancel harus menghentikan pekerjaan sedekat mungkin dengan batas aman:

```text
UI Cancel
 ↓
CancellationToken
 ↓
HTTP request cancellation
 ↓
Stop pending TTS
 ↓
Terminate local FFmpeg process
 ↓
Clean temporary artifacts
```

Jangan membatalkan proses secara kasar jika dapat meninggalkan corrupted output.

---

# 26. Error Handling UX

Setiap error harus punya:

```text
Title
Human-readable explanation
Suggested action
Technical details expandable
Retry action when appropriate
```

Contoh:

> Tidak dapat membuat suara pada bagian 18.
> Jaringan atau layanan AI sementara bermasalah.
> [Coba Lagi]

Jangan menampilkan raw JSON API sebagai pesan utama pengguna.

---

# 27. Security Requirements

## SEC-001 — API key

- Tidak boleh ditanam dalam binary.
- Tidak boleh masuk git.
- Tidak boleh masuk crash log.
- Tidak boleh ditampilkan di UI tanpa masking.

## SEC-002 — Path safety

- Gunakan PathBuf.
- Hindari shell interpolation.
- Sanitize output filename.

## SEC-003 — Temporary files

- Buat direktori temporary unik per job.
- Hapus setelah job selesai.
- Hapus saat startup untuk orphan temp directories yang aman dihapus.

## SEC-004 — Logs

Jangan mencatat:

- audio bytes.
- API key.
- full transcript secara default.
- raw user secrets.

## SEC-005 — Privacy disclosure

UI settings/about harus menjelaskan:

> Audio yang diproses dengan Gemini dikirim ke layanan Google sesuai mode API yang digunakan. Aplikasi ini menyediakan pengaturan untuk menghapus salinan lokal dan tidak mengklaim bahwa pemrosesan cloud bersifat offline.

---

# 28. Data Retention

Default:

```text
Original audio: keep locally until user deletes or job cleanup policy applies
Temporary AI artifacts: delete after job
TTS segment files: delete after final export unless debug mode
Transcript cache: optional, user-controlled
Final output: keep until user deletes
```

Privacy mode:

```text
☑ Delete temporary files after completion
☑ Do not keep transcript cache
```

---

# 29. Settings

## General

- Default output directory.
- Auto cleanup.
- Remember last language pair.

## AI

- Gemini API key.
- Model selection: Auto / Advanced.
- Concurrency.
- Retry count.

## Audio

- Output format.
- MP3 bitrate.
- Normalize volume.
- Preserve metadata.

## Translation

- Formality.
- Preserve names.
- Technical vocabulary.
- Literal ↔ Natural slider (future).

## Voice

- Default voice.
- Speaking pace.
- Style.

---

# 30. Accessibility

Minimum:

- Keyboard navigation.
- Visible focus state.
- Screen reader labels.
- No color-only status indication.
- Accessible progress descriptions.
- Buttons with explicit labels.
- Error messages accessible.
- Minimum practical contrast.

Use native GTK/Libadwaita semantics whenever possible.

---

# 31. Localization of the App

Aplikasi sendiri harus mendukung i18n, terpisah dari language translation engine.

MVP:

- English
- Indonesian

Future:

- Japanese
- Korean
- Spanish
- etc.

Jangan mencampur:

```text
UI locale
```

dengan:

```text
Audio source/target locale
```

---

# 32. Performance Requirements

Target UX pada desktop normal:

- UI startup cepat.
- UI tidak freeze selama AI processing.
- Main thread tidak menjalankan blocking IO.
- Audio processing dijalankan sebagai background task/process.
- Memory usage harus tetap bounded oleh chunking.

Tidak ada target “real-time” untuk batch translation MVP.

Target perceived behavior:

```text
Upload → progress appears immediately
Progress updates every stage
UI remains responsive
Result appears immediately after validation
```

---

# 33. Cost-Aware Architecture

Model selection harus dapat diubah per task.

Default:

```text
Transcription → dedicated transcription model
Translation → Flash-Lite class model
TTS → Flash TTS
```

Jangan menggunakan model besar untuk pekerjaan yang bisa dilakukan model kecil.

Google pricing saat ini menunjukkan `gemini-3.1-flash-lite` sebagai model yang sangat cost-efficient dan memang ditujukan untuk translation/high-volume workloads. Harga model dapat berubah, sehingga aplikasi tidak boleh meng-hardcode estimasi biaya sebagai fakta permanen. citehttps://ai.google.dev/gemini-api/docs/pricing

MVP dapat menampilkan **estimated cost** hanya jika pricing metadata tersedia secara up-to-date; jika tidak, tampilkan “Estimated usage” tanpa menyatakan harga tetap.

---

# 34. Model Registry

Jangan menyebar model IDs ke seluruh codebase.

Gunakan config:

```toml
[models]
transcriber = "gemini-3.5-transcribe"
translator = "gemini-3.1-flash-lite"
tts = "gemini-3.1-flash-tts-preview"
```

atau typed configuration.

Model IDs dapat berubah; model registry harus menjadi satu-satunya source of truth.

---

# 35. Capability Registry

```rust
struct ModelCapabilities {
    transcription: bool,
    translation: bool,
    tts: bool,
    speaker_diarization: bool,
    word_timestamps: bool,
    multi_speaker_tts: bool,
    streaming: bool,
}
```

UI harus membaca capability registry.

Contoh:

```text
Target language
  English ✓
  Indonesian ✓
  Javanese ✓
  XYZ — unavailable with selected voice
```

---

# 36. Quality Gates

Job tidak boleh menjadi COMPLETED jika:

- output file tidak dapat dibaca.
- output duration = 0.
- audio stream tidak valid.
- segment wajib hilang.
- translator menghasilkan empty content.
- TTS menghasilkan corrupted bytes.

Pipeline output validation:

```text
ffprobe
→ verify duration
→ verify codec
→ verify channels
→ verify sample rate
→ verify file size > 0
```

---

# 37. Quality Metrics

Sistem sebaiknya mengumpulkan metrik lokal per job tanpa menyimpan audio mentah.

Contoh:

```text
transcription_latency_ms
translation_latency_ms
tts_latency_ms
export_latency_ms
total_latency_ms
segments_total
segments_failed
retry_count
output_duration_ms
```

Future metrics:

- translation edit distance.
- human quality rating.
- pronunciation issue rate.
- timing deviation.

---

# 38. Observability

Log level:

```text
ERROR
WARN
INFO
DEBUG
TRACE
```

Default:

```text
INFO
```

Structured log fields:

```text
job_id
stage
segment_id
provider
model
latency_ms
retry_count
status
```

Never log API keys or complete audio payloads.

---

# 39. Crash Safety

Ketika aplikasi mati di tengah job:

1. job state disimpan secara atomic.
2. temp artifacts tetap memiliki manifest.
3. startup melakukan reconciliation.
4. job dapat ditandai `INTERRUPTED`.
5. user bisa Resume atau Discard.

Atomic persistence:

```text
write .tmp
fsync if appropriate
rename atomically
```

---

# 40. Local Project Manifest

Contoh:

```json
{
  "schema_version": 1,
  "project_id": "p_123",
  "input": {
    "path": "/home/user/Music/podcast.mp3",
    "sha256": "..."
  },
  "source_language": "id",
  "target_language": "en",
  "voice_profile": "default-en",
  "created_at": "...",
  "pipeline_version": "1.0.0"
}
```

`pipeline_version` penting agar hasil lama dapat direproduksi atau dimigrasikan.

---

# 41. Packaging Linux

## Primary

**RPM package** untuk Fedora.

## Secondary

**Flatpak** untuk distribusi Linux yang lebih luas.

## Runtime dependencies

Pastikan strategi FFmpeg jelas:

- system dependency, atau
- bundled/runtime dependency sesuai distribusi.

Jangan diam-diam mengunduh binary executable dari internet saat runtime.

---

# 42. Fedora UX Integration

Target integrasi:

- desktop application metadata.
- `.desktop` launcher.
- application icon.
- MIME associations untuk audio.
- open-with integration.
- notifications.
- GNOME style.
- system settings conventions.

Future:

- right-click “Translate with AudioDub”.
- drag file ke application launcher.
- tray/background mode.

---

# 43. CLI Companion (Recommended but Optional MVP)

Walaupun produk utama GUI, buat architecture yang memungkinkan CLI:

```bash
audiodub translate input.mp3 \
  --target en \
  --output output.mp3
```

CLI sangat berguna untuk:

- automation.
- batch processing.
- debugging.
- power users.
- CI.

GUI dan CLI harus memanggil application layer yang sama.

---

# 44. Project Structure

```text
src/
├── main.rs
├── app.rs
│
├── domain/
│   ├── audio.rs
│   ├── language.rs
│   ├── transcript.rs
│   ├── translation.rs
│   ├── synthesis.rs
│   └── job.rs
│
├── application/
│   ├── create_job.rs
│   ├── run_job.rs
│   ├── cancel_job.rs
│   ├── resume_job.rs
│   └── services/
│
├── infrastructure/
│   ├── gemini/
│   │   ├── client.rs
│   │   ├── transcribe.rs
│   │   ├── translate.rs
│   │   └── tts.rs
│   ├── ffmpeg/
│   ├── filesystem/
│   ├── secrets/
│   └── logging/
│
├── ui/
│   ├── window.rs
│   ├── dropzone.rs
│   ├── language_picker.rs
│   ├── progress.rs
│   ├── result.rs
│   ├── history.rs
│   └── settings.rs
│
└── config/
```

---

# 45. Testing Strategy

## Unit tests

Wajib untuk:

- language registry.
- capability matrix.
- state machine.
- segment merging.
- timestamp calculations.
- filename sanitizer.
- retry classification.
- config parsing.

## Integration tests

Mock provider untuk:

```text
upload
transcribe
translate
TTS
```

Tanpa memanggil Gemini pada unit test.

## Golden audio tests

Simpan test fixtures pendek.

Validasi:

- output playable.
- correct duration range.
- correct codec.
- expected segment count.

## API contract tests

Jalankan terhadap Gemini hanya di test environment / manually triggered integration suite.

---

# 46. Acceptance Test — Core MVP

### Scenario A

Input:

```text
hello.mp3
language: English
target: Indonesian
```

Expected:

```text
Transcribe ✓
Translate ✓
TTS ✓
Export ✓
Output exists ✓
Output playable ✓
```

### Scenario B

```text
podcast-id.mp3
source: Auto
target: English
```

Expected:

```text
Detected: Indonesian
```

### Scenario C

Two speakers.

Expected:

```text
Speaker 1 → Voice A
Speaker 2 → Voice B
```

### Scenario D

TTS segment fails once.

Expected:

```text
retry
resume
no duplicate completed segment
```

### Scenario E

Application closes during TTS.

Expected:

```text
job resumes from persisted state
```

---

# 47. Definition of Done — MVP

MVP dianggap selesai jika semua terpenuhi:

- [ ] Fedora 44+ dapat menjalankan aplikasi.
- [ ] Native GTK4/Libadwaita UI.
- [ ] Drag-and-drop audio.
- [ ] File chooser.
- [ ] Auto language detection.
- [ ] Source language override.
- [ ] Target language selection.
- [ ] Multi-language registry.
- [ ] Gemini transcription integration.
- [ ] Gemini translation integration.
- [ ] Gemini TTS integration.
- [ ] Single speaker workflow.
- [ ] Two-speaker workflow.
- [ ] Timestamp-aware segments.
- [ ] Segment retry.
- [ ] Job cancellation.
- [ ] Job resume.
- [ ] FFmpeg local export.
- [ ] MP3 output.
- [ ] WAV output optional.
- [ ] Output validation.
- [ ] API key secure handling.
- [ ] Error UX.
- [ ] Logging.
- [ ] Unit tests.
- [ ] Integration tests.
- [ ] RPM build.
- [ ] README installation guide.

---

# 48. MVP Implementation Tasks

## Phase 1 — Project foundation

- [ ] Create Cargo project.
- [ ] Configure Rust edition/toolchain.
- [ ] Add GTK4.
- [ ] Add Libadwaita.
- [ ] Create application window.
- [ ] Add logging.
- [ ] Add config.
- [ ] Add test framework.

## Phase 2 — Audio layer

- [ ] Add file inspection.
- [ ] Validate MIME type.
- [ ] Validate duration.
- [ ] Integrate FFmpeg/ffprobe.
- [ ] Create temp job directories.
- [ ] Output exporter.

## Phase 3 — Gemini adapter

- [ ] Create provider trait.
- [ ] Gemini client.
- [ ] File upload.
- [ ] transcription adapter.
- [ ] translation adapter.
- [ ] TTS adapter.
- [ ] error mapping.

## Phase 4 — Domain pipeline

- [ ] Language registry.
- [ ] Job model.
- [ ] State machine.
- [ ] Transcript model.
- [ ] Translation model.
- [ ] Segment model.
- [ ] retry policy.
- [ ] cancellation.
- [ ] resume.

## Phase 5 — UI

- [ ] Dropzone.
- [ ] Source language picker.
- [ ] Target language picker.
- [ ] Voice picker.
- [ ] Progress screen.
- [ ] Result player.
- [ ] Save dialog.
- [ ] Error dialogs.
- [ ] Settings.

## Phase 6 — Quality

- [ ] Automated tests.
- [ ] Failure simulation.
- [ ] Large file test.
- [ ] Multi-language test.
- [ ] Two-speaker test.
- [ ] Output validation.
- [ ] Performance profiling.

## Phase 7 — Packaging

- [ ] RPM spec.
- [ ] desktop file.
- [ ] icon.
- [ ] MIME association.
- [ ] installation docs.
- [ ] optional Flatpak manifest.

---

# 49. AI Coding Agent Instructions

Dokumen ini dirancang untuk diberikan kepada AI coding agent.

AI agent wajib mengikuti aturan berikut:

## Rule 1 — Build incrementally

Jangan menghasilkan seluruh aplikasi sebagai satu patch raksasa.

Urutan:

```text
foundation
→ UI shell
→ audio abstraction
→ Gemini adapter
→ pipeline
→ progress
→ result
→ tests
→ packaging
```

## Rule 2 — Compile after meaningful changes

Setelah perubahan besar jalankan:

```bash
cargo check
cargo test
```

Lalu:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Untuk release:

```bash
cargo build --release
```

## Rule 3 — Never invent APIs

Jangan membuat signature Gemini berdasarkan asumsi.

Selalu mengikuti dokumentasi Gemini API yang berlaku untuk model/version yang dipilih.

## Rule 4 — Isolate provider code

Gemini SDK/HTTP types tidak boleh bocor ke domain.

## Rule 5 — No secrets in source

Jangan membuat:

```rust
const GEMINI_API_KEY: &str = "...";
```

## Rule 6 — Do not block GTK UI

Semua network/audio-heavy task harus async/background.

## Rule 7 — Fail gracefully

Semua external operation harus menghasilkan typed error.

## Rule 8 — Preserve user data

Jangan menghapus original file user.

## Rule 9 — Keep dependencies justified

Jangan menambahkan crate hanya karena populer.

Setiap dependency baru harus memiliki alasan teknis.

## Rule 10 — Do not over-engineer

MVP tidak membutuhkan:

```text
microservices
Kubernetes
Redis
Kafka
PostgreSQL server
```

---

# 50. Suggested Cargo Dependencies

Daftar awal; versi exact harus dipilih berdasarkan kompatibilitas Fedora 44 dan crates yang aktif saat implementasi.

```toml
[dependencies]
gtk4 = "..."
adw = "..."
tokio = { version = "...", features = ["rt-multi-thread", "macros", "process", "fs", "sync"] }
serde = { version = "...", features = ["derive"] }
serde_json = "..."
reqwest = { version = "...", features = ["json", "multipart", "stream"] }
thiserror = "..."
anyhow = "..."
tracing = "..."
tracing-subscriber = "..."
chrono = { version = "...", features = ["serde"] }
uuid = { version = "...", features = ["v4", "serde"] }
sha2 = "..."
```

**Catatan:** jangan copy version placeholder mentah. AI coding agent harus memilih versi kompatibel terkini ketika scaffolding.

---

# 51. Example API Flow

## Upload

```text
POST / Gemini File API
```

Input:

```text
audio.mp3
```

## Transcription

```text
model = gemini-3.5-transcribe
```

Expected domain output:

```text
Transcript {
  language,
  segments[]
}
```

## Translation

```text
model = gemini-3.1-flash-lite
```

## TTS

```text
model = gemini-3.1-flash-tts-preview
```

Output raw audio harus segera dinormalisasi/ditulis ke file yang dapat diproses FFmpeg.

---

# 52. Prompt Templates

## Transcription prompt

```text
Transcribe the supplied audio.

Requirements:
- Detect spoken language automatically unless a language is explicitly supplied.
- Preserve speaker boundaries when diarization is enabled.
- Produce timestamps suitable for downstream audio alignment.
- Preserve names, technical terms and numbers accurately.
- Do not translate the speech.
- Return structured segment data.
```

## Translation prompt

```text
Translate the following spoken transcript from {SOURCE_LANGUAGE}
to {TARGET_LANGUAGE}.

Requirements:
- Preserve meaning and intent.
- Make the translation natural for spoken audio.
- Preserve speaker identity and segment IDs.
- Preserve names, numbers, dates, units and technical terminology.
- Do not add facts.
- Keep each translated segment concise enough for natural dubbing.
- Return structured output only.
```

## TTS prompt

```text
Synthesize the following transcript as natural spoken {TARGET_LANGUAGE}.

Audio profile:
- clear
- natural
- conversational
- appropriate pacing

Director notes:
- preserve emotional intent
- speak naturally rather than reading metadata
- do not say the instructions aloud

Spoken transcript begins:
---
{TEXT}
---
```

---

# 53. Content & Safety Policy

MVP tidak perlu membuat keputusan moral kompleks sendiri, tetapi harus:

- menghormati safety/error response provider.
- tidak mengklaim hasil sebagai human translation.
- tidak menyebut “voice clone” kecuali memang menggunakan capability voice cloning yang sah.
- tidak menghilangkan copyright ownership dari user.
- memperingatkan user bahwa hak atas audio dan penggunaan hasil tetap menjadi tanggung jawab user.

---

# 54. Copyright & User Rights UX

Pada About/Help:

> Pastikan Anda memiliki hak atau izin yang diperlukan untuk memproses dan mendistribusikan audio yang Anda unggah. AudioDub AI tidak menentukan kepemilikan atau lisensi materi Anda.

---

# 55. Product Metrics

MVP metrics:

### Activation

```text
percentage of users who successfully create first output
```

### Completion

```text
completed jobs / started jobs
```

### Failure

```text
failed jobs / started jobs
```

### Time to result

```text
total processing time / source duration
```

### Quality

Human rating:

```text
1–5 naturalness
1–5 translation accuracy
1–5 intelligibility
```

Tidak perlu analytics cloud pada MVP.

---

# 56. Success Criteria

Produk dikatakan berhasil jika pengguna yang belum memahami AI pipeline dapat melakukan:

```text
Drop MP3
→ Choose language
→ Click Translate
→ Listen
```

tanpa terminal dan tanpa memahami detail AI.

Technical success:

```text
Native app
+ stable pipeline
+ multi-language
+ resumable jobs
+ valid MP3
+ good UX
```

---

# 57. Phase 2 Roadmap

Setelah MVP stabil:

## P2.1 Subtitle

Tambahkan:

```text
SRT
VTT
TXT
```

## P2.2 Video

```text
MP4
MKV
MOV
```

AudioDub berubah menjadi video dubbing tool.

## P2.3 Original background preservation

Tambahkan source separation:

```text
voice
music
ambience
```

## P2.4 Better translation control

```text
formal
neutral
casual
creator style
```

## P2.5 Translation editor

UI segment editor:

```text
Original | Translation | Start | End | Speaker
```

User dapat mengoreksi sebelum TTS.

---

# 58. Phase 3 — Real-Time Mode

Arsitektur dapat berkembang menjadi:

```text
Microphone
 ↓
Live speech recognition
 ↓
Real-time translation
 ↓
Voice generation
 ↓
Speaker output
```

Gunakan model Live/real-time yang sesuai, bukan memaksa batch architecture.

Google saat ini mendokumentasikan `gemini-3.5-live-translate-preview` sebagai low-latency speech-to-speech translation dengan dukungan 70+ languages; capability ini cocok sebagai future mode, bukan core batch MVP. citehttps://ai.google.dev/gemini-api/docs/pricing

---

# 59. Phase 4 — SaaS / Team Version

Future cloud architecture:

```text
Desktop
   ↓
Cloud API
   ↓
Job Queue
   ↓
Workers
   ├── Transcription
   ├── Translation
   ├── TTS
   └── Audio Assembly
```

Tetapi jangan memasukkan arsitektur tersebut ke MVP desktop.

---

# 60. International Engineering Standards Alignment

PRD ini memakai prinsip yang selaras dengan praktik requirements engineering modern, terutama:

- **ISO/IEC/IEEE 29148** untuk requirements engineering.
- **ISO/IEC 25010** untuk quality characteristics.
- Secure-development practices untuk secret management, input validation, logging, dependency control, dan failure handling.

Dalam implementasi, setiap requirement sebaiknya mempunyai:

```text
ID
Description
Priority
Acceptance Criteria
Dependencies
Verification Method
```

Contoh:

```text
REQ-AUDIO-001
The application shall accept MP3 input.
Priority: MUST
Verification: automated integration test
```

---

# 61. Requirements Priority Model

Gunakan MoSCoW:

## MUST

- Native GUI.
- Audio upload/drop.
- Multi-language source/target.
- transcription.
- translation.
- TTS.
- FFmpeg export.
- MP3 output.
- retry.
- error handling.
- API key security.

## SHOULD

- speaker awareness.
- local history.
- resume.
- WAV output.
- settings.
- RPM + Flatpak.

## COULD

- subtitles.
- video.
- source separation.
- translation editor.
- CLI.

## WON'T in MVP

- live conversation.
- SaaS backend.
- billing.
- voice cloning.

---

# 62. Requirement Traceability Matrix

| ID | Requirement | Priority | Verification |
|---|---|---|---|
| REQ-UI-001 | App launches as native GTK application | MUST | Manual + integration |
| REQ-AUDIO-001 | Accept MP3 | MUST | Automated |
| REQ-AUDIO-002 | Validate media locally | MUST | Unit + integration |
| REQ-AI-001 | Transcribe input | MUST | Integration |
| REQ-AI-002 | Detect source language | MUST | Integration |
| REQ-AI-003 | Translate to selected target | MUST | Integration |
| REQ-AI-004 | Generate TTS | MUST | Integration |
| REQ-AUDIO-003 | Build final MP3 | MUST | Automated |
| REQ-JOB-001 | Persist job state | SHOULD | Integration |
| REQ-JOB-002 | Resume interrupted job | SHOULD | Integration |
| REQ-JOB-003 | Retry transient failure | MUST | Failure injection test |
| REQ-SEC-001 | Protect API key | MUST | Security review |
| REQ-SEC-002 | Prevent unsafe shell invocation | MUST | Code review |
| REQ-I18N-001 | Dynamic language registry | MUST | Unit |
| REQ-PKG-001 | Fedora packaging | MUST | Release test |

---

# 63. Release Checklist

## Code

- [ ] No compile warnings.
- [ ] Clippy passes.
- [ ] Tests pass.
- [ ] Dependency audit reviewed.

## Gemini

- [ ] Model IDs verified against current docs.
- [ ] API errors mapped.
- [ ] Retry logic tested.
- [ ] Rate limits considered.

## Audio

- [ ] MP3 output verified.
- [ ] WAV output verified.
- [ ] No corrupted segments.
- [ ] ffprobe validation passes.

## UX

- [ ] Keyboard navigation.
- [ ] Error states.
- [ ] Cancellation.
- [ ] Progress.
- [ ] Empty state.

## Packaging

- [ ] RPM installs cleanly.
- [ ] Desktop launcher works.
- [ ] MIME handling works.
- [ ] Uninstall cleanly.

---

# 64. Reference Architecture Diagram

```text
                    ┌──────────────────────┐
                    │     GTK4 / Adwaita   │
                    │       Native UI      │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │    Application Core   │
                    │  Job + State Machine  │
                    └───────┬───────┬──────┘
                            │       │
                ┌───────────▼─┐   ┌▼──────────────┐
                │ Gemini Port │   │ Audio Port    │
                └──────┬──────┘   └──────┬────────┘
                       │                 │
            ┌──────────┼─────────┐       │
            │          │         │       │
            ▼          ▼         ▼       ▼
       Transcribe   Translate   TTS   FFmpeg
            │          │         │       │
            └──────────┴────┬────┴───────┘
                            ▼
                      Output Validator
                            │
                            ▼
                         MP3/WAV
```

---

# 65. Final Technical Recommendation

## Chosen stack

```text
Language: Rust
UI: GTK4 + Libadwaita
Async: Tokio
AI: Google Gemini API
STT: gemini-3.5-transcribe
Translation: gemini-3.1-flash-lite
TTS: gemini-3.1-flash-tts-preview
Audio: FFmpeg + ffprobe
Storage: filesystem + JSON/TOML manifests
Packaging: RPM first, Flatpak second
```

## Why this architecture

```text
Native Linux
        +
Lightweight
        +
Safe concurrency
        +
Simple deployment
        +
Gemini AI capability
        +
Local audio processing
        +
Multi-language by design
        +
Future-proof provider abstraction
```

---

# 66. Important Implementation Notes from Current Research

1. `gemini-3.5-transcribe` is the dedicated current Gemini transcription model and supports automatic language identification across 85+ locales plus speaker diarization and word timestamps. citehttps://ai.google.dev/gemini-api/docs/transcribe

2. `gemini-3.1-flash-lite` is the current GA Flash-Lite model for cost-efficient high-volume/translation workloads. The preview version was shut down on 25 May 2026 and must not be used. citehttps://ai.google.dev/gemini-api/docs/changelog

3. `gemini-3.1-flash-tts-preview` supports single- and multi-speaker TTS, multilingual speech, controllable style/pace/tone, and streaming. The official docs specifically advise splitting long outputs into smaller chunks because longer outputs can drift in quality/consistency. citehttps://ai.google.dev/gemini-api/docs/speech-generation

4. Gemini File API is designed for larger files and documents up to 2 GB per file, with standard uploaded files retained for 48 hours. citehttps://ai.google.dev/gemini-api/docs/file-input-methods

5. GTK4 supports drag-and-drop and modern asynchronous file selection; Libadwaita supplies modern adaptive GNOME components. citehttps://docs.gtk.org/gtk4/drag-and-drop.html citehttps://gnome.pages.gitlab.gnome.org/libadwaita/

---

# 67. References

- Google AI for Developers — Audio transcription: https://ai.google.dev/gemini-api/docs/transcribe
- Google AI for Developers — Gemini 3.5 Transcribe model: https://ai.google.dev/gemini-api/docs/models/gemini-3.5-transcribe
- Google AI for Developers — Speech generation / TTS: https://ai.google.dev/gemini-api/docs/speech-generation
- Google AI for Developers — Gemini 3.1 Flash-Lite: https://ai.google.dev/gemini-api/docs/models/gemini-3.1-flash-lite
- Google AI for Developers — Pricing: https://ai.google.dev/gemini-api/docs/pricing
- Google AI for Developers — File input methods: https://ai.google.dev/gemini-api/docs/file-input-methods
- Google AI for Developers — API changelog: https://ai.google.dev/gemini-api/docs/changelog
- GTK4 documentation: https://docs.gtk.org/gtk4/
- GTK Rust bindings: https://gtk-rs.org/
- Libadwaita documentation: https://gnome.pages.gitlab.gnome.org/libadwaita/
- ISO/IEC/IEEE 29148 — Requirements engineering standard: https://www.iso.org/standard/72089.html
- ISO/IEC 25010 — Product quality model: https://www.iso.org/standard/78176.html

---

# 68. One-Sentence Product Definition

> **AudioDub AI is a native Linux desktop application that turns spoken audio from one language into a natural, timestamp-aware spoken version in another language through a resilient Gemini-powered transcription, translation, and TTS pipeline with local FFmpeg assembly.**

---

# 69. AI Agent Kickoff Instruction

Gunakan dokumen ini sebagai **single source of truth**. Implementasikan aplikasi secara bertahap, mulai dari foundation dan UI shell, kemudian audio abstraction, Gemini adapters, pipeline orchestration, progress/resume/cancel, output validation, tests, dan packaging Fedora.

Sebelum mengunci model ID, endpoint, SDK API, atau capability language, verifikasi dokumentasi Gemini resmi terbaru. Jangan mengganti model atau framework hanya karena preferensi pribadi; setiap perubahan harus memiliki alasan teknis dan dicatat sebagai Architecture Decision Record (ADR).

Prioritas utama produk adalah:

```text
Correctness
→ Reliability
→ UX
→ Performance
→ Cost
→ Extra features
```

