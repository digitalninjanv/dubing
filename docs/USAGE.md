# Panduan Lengkap Penggunaan AudioDub AI

Dokumen ini menyediakan panduan operasional terperinci untuk pengguna dan pengembang yang menggunakan **AudioDub AI** pada sistem operasi Linux (Fedora, Ubuntu, Debian, Arch, dsb.).

---

## 1. Persiapan & Konfigurasi API Key

AudioDub AI memanfaatkan API Google Gemini untuk pengenalan suara (*STT*), penerjemahan (*Translation*), dan sintesis vokal (*TTS*).

### Mendapatkan API Key
1. Kunjungi [Google AI Studio](https://aistudio.google.com/).
2. Buat akun atau masuk dengan akun Google Anda.
3. Klik tombol **Get API key** lalu buat API key baru.

### Mengonfigurasi API Key di AudioDub AI
Terdapat 3 cara untuk menyediakan API key ke aplikasi:

1. **Melalui Pengaturan GUI (Direkomendasikan untuk Desktop):**
   - Buka aplikasi AudioDub AI.
   - Klik ikon gir (*Preferences*) pada pojok kanan atas HeaderBar.
   - Pada bagian **Google Gemini API**, masukkan API key Anda ke dalam kolom teks **API Key**.
   - Kunci akan otomatis disimpan ke sistem *FreeDesktop Secret Service* (GNOME Keyring / KWallet) Anda dengan enkripsi tingkat OS.
   - Jika Secret Service tidak aktif pada desktop Anda, kunci akan disimpan dengan aman di file fallback `~/.config/audiodub/credentials` dengan izin berkas Unix `0600` (hanya dapat dibaca/ditulis oleh akun Anda).

2. **Melalui Environment Variable (Direkomendasikan untuk CLI):**
   Tambahkan environment variable sebelum menjalankan perintah:
   ```bash
   export GEMINI_API_KEY="AIzaSyYourSecretKeyHere"
   ```
   Untuk membuatnya permanen pada sesi shell Anda, tambahkan baris di atas ke file `~/.bashrc` atau `~/.zshrc`.

3. **Melalui File Konfigurasi Manual:**
   Buat file `~/.config/audiodub/credentials` dengan isi:
   ```ini
   gemini_api_key=AIzaSyYourSecretKeyHere
   ```
   Pastikan izin berkas dikunci:
   ```bash
   chmod 600 ~/.config/audiodub/credentials
   ```

---

## 2. Penggunaan Antarmuka Desktop (GUI)

### Antarmuka Utama (Translate View)
1. **Dropzone:**
   - Anda dapat menyeret (*drag and drop*) file audio dari file manager (Nautilus, Dolphin, Nemo, Thunar) langsung ke area dropzone.
   - Atau klik tombol **Choose a File…** untuk membuka dialog pemilih file asli GTK4.
   - Format yang didukung: MP3, WAV, M4A, AAC, OGG, FLAC, WebM, Opus.
2. **Pengaturan Bahasa:**
   - **Source Language:** Pilih `Auto Detect` agar model `gemini-3.5-transcribe` mendeteksi bahasa lisan secara otomatis, atau pilih bahasa yang Anda ketahui.
   - **Target Language:** Pilih bahasa tujuan dubbing (tersedia 85+ bahasa termasuk Indonesian, English, Japanese, German, French, Spanish, Chinese, Korean, Arabic, dll.).
3. **Validasi Instan:**
   - Tombol **Translate & Dub Audio** hanya akan aktif setelah file audio yang valid dipilih.
   - Jika Anda memilih bahasa sumber dan target yang identik, sistem akan memberikan notifikasi toast agar Anda memilih bahasa tujuan yang berbeda.

### Tampilan Pemrosesan (Progress View)
Setelah tombol ditekan, tampilan akan beralih ke Progress View:
- Baris status interaktif menampilkan tahapan pipa saat ini:
  1. `Validating` (Memeriksa durasi, integritas file, dan codec via ffprobe)
  2. `Uploading` (Mengunggah audio ke Gemini Files API)
  3. `Transcribing` (Mengekstrak ucapan dan speaker diarization)
  4. `Translating` (Menerjemahkan kalimat secara kontekstual dalam bentuk batch)
  5. `Synthesizing` (Menghasilkan audio suara baru dengan indikator: *Synthesized segment X of Y*)
  6. `Aligning` (Menyelaraskan celah keheningan dan tempo bicara)
  7. `Exporting` (Melakukan encode final ke format MP3)
- **Tombol Cancel:** Anda dapat membatalkan proses kapan saja. Pembatalan bersifat instan dan otomatis membersihkan file segmen sementara.

### Tampilan Hasil (Result View)
Setelah dubbing selesai:
- Menampilkan rincian durasi audio dan ukuran berkas yang dihasilkan.
- Menampilkan informasi jalur penyimpanan file (`Saved to: ~/.local/share/audiodub/outputs/...`).
- **Play Audio:** Membuka pemutar media default Linux untuk langsung mendengarkan hasilnya.
- **Open Containing Folder:** Membuka folder output di file manager Anda.
- **Quality Notice:** Jika ada segmen percakapan yang dipercepat (time-stretched) atau beririsan, kotak peringatan kuning akan muncul menjelaskan rincian segmen tersebut.
- **Start Another Translation:** Mengembalikan tampilan ke layar utama untuk memproses file baru.

### Tampilan Riwayat (History View)
- Klik tombol riwayat (ikon jam/dokumen) di kiri atas HeaderBar.
- Menampilkan daftar semua pekerjaan dubbing yang pernah dilakukan, lengkap dengan nama file sumber, pasangan bahasa, status, dan waktu pembuatan.
- Klik tombol kembali (*back arrow*) untuk kembali ke layar kerja utama.

---

## 3. Penggunaan Antarmuka Terminal (CLI)

AudioDub AI dilengkapi antarmuka baris perintah (*CLI*) bawaan yang cepat dan ramah skrip.

### Menampilkan Bantuan
```bash
audiodub --help
```

### Sintaks Perintah Terjemahan
```bash
audiodub translate <input-file> --target <target-lang> [--source <source-lang>] [--output <output-file>]
```

### Argumen:
- `<input-file>`: Jalur ke file audio sumber (wajib).
- `--target` atau `-t`: Kode bahasa target (mis. `en`, `id`, `ja`, `es`, `de`, dll.) (wajib).
- `--source` atau `-s`: Kode bahasa sumber (opsional, default: `auto`).
- `--output` atau `-o`: Jalur penyimpanan berkas MP3 hasil dubbing (opsional).

### Contoh Perintah:
```bash
# Dubbing podcast bahasa Indonesia ke bahasa Inggris
audiodub translate podcast.mp3 --source id --target en --output dub_english.mp3

# Dubbing wawancara dengan deteksi otomatis ke bahasa Jepang
audiodub translate interview.m4a --target ja --output interview_nihongo.mp3

# Dubbing rekaman suara bahasa Jerman ke bahasa Indonesia
audiodub translate meeting.ogg --source de --target id
```

---

## 4. Mekanisme Multi-Speaker & Profil Suara

AudioDub AI memanfaatkan deteksi pembicara (*speaker diarization*) dari model `gemini-3.5-transcribe`:
- Segmen percakapan secara otomatis dilabeli dengan ID pembicara (mis. `Speaker 1`, `Speaker 2`).
- Sistem mengalokasikan profil suara alami yang berbeda untuk setiap pembicara dari kelompok suara prebuilt Gemini TTS (`Aoede`, `Kore`, `Fenrir`, `Puck`).
- Suara utama disesuaikan dengan bahasa target yang dipilih (misalnya *Aoede* untuk Bahasa Indonesia, *Kore* untuk Bahasa Inggris, *Fenrir* untuk Bahasa Jerman).
- Pembicara kedua dialokasikan suara alternatif dari kelompok tersebut agar dialog percakapan terdengar kontras dan natural layaknya dua orang yang berbeda.

---

## 5. Pemecahan Masalah (Troubleshooting FAQ)

### Masalah 1: `ffmpeg: command not found`
- **Penyebab:** Binary `ffmpeg` atau `ffprobe` belum terpasang di sistem operasi.
- **Solusi:**
  - Fedora: `sudo dnf install ffmpeg`
  - Ubuntu / Debian: `sudo apt install ffmpeg`
  - Arch: `sudo pacman -S ffmpeg`

### Masalah 2: `Gemini API key not found`
- **Penyebab:** Belum ada API key yang dikonfigurasi.
- **Solusi:** Buka Settings pada aplikasi GUI dan masukkan API key Anda, atau jalankan `export GEMINI_API_KEY="..."` pada terminal sebelum menggunakan CLI.

### Masalah 3: `Error 429: Too Many Requests`
- **Penyebab:** Anda telah melampaui batas kuota permintaan per menit (RPM) pada tier gratis Google AI Studio.
- **Solusi:** AudioDub AI secara otomatis melakukan retry dengan *exponential backoff*. Jika kuota harian habis, tunggu hingga kuota di-reset atau gunakan API key dengan kuota berbayar (Pay-as-you-go).

### Masalah 4: Peringatan Kualitas (*Quality Warning: clamped to 1.25x*)
- **Penyebab:** Kalimat terjemahan bahasa target memiliki jumlah suku kata yang jauh lebih banyak daripada rekaman ucapan asli (misal: kalimat pendek bahasa Inggris diterjemahkan menjadi kalimat panjang dalam bahasa Jerman).
- **Perilaku Sistem:** AudioDub AI sengaja membatasi percepatan suara maksimal 1.25× agar suara tidak melengking tidak wajar (*chipmunk effect*). Audio tetap diekspor dengan utuh dan peringatan kualitas ditampilkan agar pengguna dapat mengevaluasinya.

### Masalah 5: Direktori Sandbox & Log Lokasi
- File manifest dan cache sementara: `~/.local/share/audiodub/jobs/`
- File MP3 hasil dubbing: `~/.local/share/audiodub/outputs/`
- Konfigurasi aplikasi: `~/.config/audiodub/`
