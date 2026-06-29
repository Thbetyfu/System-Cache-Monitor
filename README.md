# Cache Advisor

Cache Advisor adalah aplikasi desktop berbasis Windows yang ditulis dalam bahasa Rust menggunakan antarmuka grafis `egui` (`eframe`). Aplikasi ini dirancang untuk memantau, menganalisis, mengarsipkan, membersihkan penyimpanan disk secara aman, mendeteksi berkas duplikat, dan memberikan saran optimasi menggunakan model kecerdasan buatan (LLM) lokal.

---

## Fitur Utama

1. **Pemindaian & Klasifikasi Penyimpanan (Scan & Classify)**
   - Menghitung ukuran direktori secara akurat hingga tingkat byte (bukan sekadar menghitung jumlah item).
   - Menilai risiko penumpukan cache berdasarkan algoritma urgensi (urgency score) dan membaginya ke dalam tiga kategori:
     - `Cache`: Aman dibersihkan kapan saja (file temp, log, cache aplikasi).
     - `Cautious`: Memerlukan konfirmasi pengguna sebelum dihapus (file build compiler).
     - `MonitorOnly`: Hanya dipantau ukurannya, tidak boleh dihapus (Docker data, VM).

2. **Pengarsipan Aman (Safe Archiving)**
   - Memindahkan folder berukuran besar dari drive sistem utama (C:) ke penyimpanan eksternal (E:).
   - Menyimpan berkas manifest pengarsipan sehingga seluruh proses pengarsipan dapat dibatalkan sewaktu-waktu (automated Undo) secara utuh.

3. **Deteksi Berkas Duplikat (Duplicate Finder)**
   - Mendeteksi file duplikat di antara semua folder yang dipantau menggunakan pencocokan hash **SHA-256**.
   - **Optimalisasi Kecepatan:** Hanya menghitung hash untuk berkas yang memiliki ukuran byte yang sama (pre-filter by size), menghindari overhead I/O yang tidak perlu.
   - **Safety Guard:** Tombol hapus instansi duplikat dinonaktifkan secara otomatis jika hanya tersisa 1 salinan berkas terakhir di dalam kelompok tersebut untuk mencegah kehilangan data secara tidak sengaja.

4. **Kecerdasan Buatan Lokal (Ask AI - Local LLM)**
   - Terintegrasi dengan model **Qwen2 1.5B (GGUF)** lokal melalui pustaka `llama-cpp-4` (v0.3.2).
   - **Efisiensi RAM:** Model hanya akan dimuat ke memori (RAM ~1GB) saat pengguna membuka tab "Ask AI", dan langsung dibebaskan (dropped) secara otomatis dari RAM begitu pengguna berpindah ke tab lain.
   - Menganalisis konteks pemindaian sistem untuk memberikan rekomendasi pembersihan yang akurat dan menjawab pertanyaan kustom pengguna.

5. **Konfigurasi Kustom (Settings TOML)**
   - Mendukung berkas `settings.toml` untuk mengatur ambang batas hari file usang (`stale_days`), lokasi path model GGUF kustom, dan daftar folder yang dipantau.

---

## Struktur Proyek

Aplikasi ini menggunakan arsitektur modular multi-crate dalam Rust Workspace:

- **`crates/core`**: Logika murni untuk pemindaian (`scanner`), perhitungan risiko (`classifier`), pengarsipan (`archive`), pencarian duplikat (`duplicates`), dan pemuatan konfigurasi (`settings`).
- **`crates/actions`**: Modul yang melakukan mutasi filesystem secara destruktif seperti penghapusan cache, penghapusan file duplikat, pengarsipan, dan undo pengarsipan.
- **`crates/llm`**: Wrapper engine LLM lokal menggunakan binding C++ `llama-cpp-4`.
- **`crates/app`**: Aplikasi GUI utama (`cache-advisor`) yang mengontrol UI thread, background workers, dan polling state.

---

## Prasyarat Kompilasi & Build

Jika Anda ingin mengompilasi fitur AI (`--features ai`), generator binding (`bindgen`) memerlukan instalasi LLVM di Windows untuk mendeteksi `libclang`:

1. Unduh dan pasang **LLVM 19** pada Windows.
2. Atur variabel lingkungan (Environment Variable) `LIBCLANG_PATH` mengarah ke folder bin LLVM Anda sebelum menjalankan kargo. Contoh via PowerShell:
   ```powershell
   $env:LIBCLANG_PATH="D:\LLVM19\bin"
   ```

---

## Cara Menjalankan

### 1. Build Standar (Tanpa Fitur AI)
Menjalankan GUI standar dengan fitur scan, archive, dan deteksi berkas duplikat:
```powershell
cargo run -p cache-advisor
```

### 2. Build Lengkap dengan Fitur AI (Local LLM)
Pastikan LLVM 19 telah terpasang dan model Qwen2 1.5B GGUF telah diunduh pada direktori yang disetel:
```powershell
$env:LIBCLANG_PATH="D:\LLVM19\bin"
cargo run -p cache-advisor --features ai
```

---

## Konfigurasi settings.toml

Salin berkas `settings.toml.example` menjadi `settings.toml` untuk menyesuaikan setelan aplikasi:

```toml
# Menentukan berapa hari file dianggap usang (stale)
stale_days = 90

[llm]
# Path kustom ke model Qwen2 GGUF Anda
model_path = "D:\\MODEL OLLAMA\\blobs\\sha256-405b56374e02b21122ae1469db646be0617c02928fd78e246723ebbb98dbca3e"

# Menentukan daftar folder kustom yang dipantau (opsional)
[[folders]]
name = "User Temp Files"
path = "%TEMP%"
tier = "cache"
note = "Per-user temp; safe to wipe."
```
Jika berkas `settings.toml` tidak ditemukan, aplikasi akan otomatis berjalan menggunakan konfigurasi default sistem yang aman (graceful fallback).
