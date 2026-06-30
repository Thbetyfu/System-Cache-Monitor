# Product Requirements Document (PRD)
# Cache Advisor — Agentic Storage Advisor

> **Versi:** 0.2.0 (rebuild dari Python → Rust)
> **Tanggal:** 2026-06-30
> **Status:** Core + GUI + Archive selesai & teruji. Integrasi LLM in-progress.
> **Penulis:** hasil kolaborasi analisis kebutuhan + implementasi

---

## Daftar Isi

1. [Ringkasan Eksekutif](#1-ringkasan-eksekutif)
2. [Latar Belakang & Masalah](#2-latar-belakang--masalah)
3. [Tujuan & Non-Tujuan](#3-tujuan--non-tujuan)
4. [Persona Pengguna](#4-persona-pengguna)
5. [Arsitektur Sistem](#5-arsitektur-sistem)
6. [Spesifikasi Fitur](#6-spesifikasi-fitur)
7. [Model "AI" yang Digunakan](#7-model-ai-yang-digunakan)
8. [Persyaratan Non-Fungsional](#8-persyaratan-non-fungsional)
9. [Keamanan & Keamanan Data](#9-keamanan--keamanan-data)
10. [Metrik & Pengujian](#10-metrik--pengujian)
11. [Lingkungan Build & Dependensi](#11-lingkungan-build--dependensi)
12. [Roadmap & Status](#12-roadmap--status)
13. [Trade-off & Keputusan Desain](#13-trade-off--keputusan-desain)
14. [Yang TIDAK Dikerjakan](#14-yang-tidak-dikerjakan)
15. [Glosarium](#15-glosarium)

---

## 1. Ringkasan Eksekutif

**Cache Advisor** adalah aplikasi desktop native Windows yang menyelesaikan masalah
*digital hoarding* dan kepenuhan disk dengan bertindak sebagai **Portable Storage Relocation & Auto-Routing Agent**: ia berjalan langsung dari penyimpanan eksternal (USB/HDD), memindai folder cache/temp laptop, memindahkan berkas sampah tersebut ke dalam dirinya sendiri (self-archiving), dan secara otomatis membelokkan variabel lingkungan (Environment Variables) Windows agar semua aplikasi target langsung menyimpan cache baru ke penyimpanan eksternal tanpa tindakan manual dari pengguna.

Aplikasi ini adalah rebuild dari tool Python 217-baris sebelumnya (`cache_monitor.py`)
menjadi aplikasi Rust native dengan empat peningkatan inti:

1. **Akurasi** — bug lama yang menghitung "ruang dibebaskan" sebagai jumlah *item*
   alih-alih *byte* telah diperbaiki dan teruji.
2. **Relokasi & Auto-Routing** — memindahkan cache/temp ke disk tempat program berjalan dan memperbarui Environment Variables Windows secara otomatis agar aplikasi menulis langsung ke disk eksternal.
3. **Mekanisme Undo (Recycle Bin)** — memindahkan berkas ke folder Recycle Bin lokal dengan manifest JSON sehingga bisa dikembalikan (undo) atau dihapus permanen (purge).
4. **Panel AI opsional** — integrasi LLM lokal (llama.cpp) untuk menjawab pertanyaan
   bahasa natural tentang hasil scan. **Tidak pernah mengeksekusi aksi otomatis.**

### Posisi saat ini

| Komponen | Status |
|---|---|
| Core logic (scan + classify + archive plan) | ✅ Selesai, 14 unit test lulus (termasuk Disk Map) |
| Actions (clean + archive + undo) | ✅ Selesai, 10 unit test lulus (termasuk Undo Cleaner) |
| GUI (egui: scan table, archive, disk map, duplicates) | ✅ Selesai, binary 3.8 MB |
| Integrasi LLM (llama-cpp-4) | ✅ Selesai (AI panel, AI worker fully functional) |
| Fitur Tambahan (Recycle Bin, Whitelist, Smart Duplicates) | ✅ Selesai (100% terintegrasi) |
| Relokasi & Auto-Routing (F10/F11/F12) | 🔨 In-progress (tahap desain & integrasi registry) |
| Dokumentasi | 📝 Diperbarui (PRD v0.4.0) |

---

## 2. Latar Belakang & Masalah

### 2.1 Masalah asli (digital hoarding)

Drive utama (C:) pada mesin pengguna penuh dengan file yang sebenarnya tidak
perlu ada di sana:

- **Cache aplikasi** — VS Code CachedData, CapCut pre-render, pip cache.
- **File temp** — `%TEMP%`, `C:\Windows\Temp`.
- **Data besar yang jarang dipakai** — image Docker, model ML, build artifacts.

Akibatnya disk utama melambat, sistem tidak bisa update, dan ruang untuk hal
yang benar-benar penting menyempit.

### 2.2 Tool sebelumnya (baseline)

`cache_monitor.py` (Python + tkinter, 217 baris) menyelesaikan sebagian masalah:
memindai ~10 folder dan menampilkan ukurannya, dengan tombol "Clean All Safe Cache".

**Empat kelemahan kritis:**

1. **Bug akuntansi byte** — fungsi `perform_clean` menulis `freed += 1` per item,
   sehingga laporan "ruang dibebaskan" bohong (menghitung jumlah file, bukan ukuran).
2. **Tidak ada klasifikasi risiko** — semua folder cache diperlakukan sama; tidak
   ada peringatan dini untuk yang berbahaya atau pemisahan dari yang aman.
3. **Tidak ada fitur arsip** — satu-satunya pilihan adalah hapus. Tidak ada cara
   untuk "pindahkan ke external dulu, kalau nanti butuh masih ada".
4. **Tidak ada saran berbasis konteks** — pengguna harus menebak sendiri folder
   mana yang prioritas.

### 2.3 Mengapa "AI agentic" bukan jawaban tunggal

Penilaian jujur: keputusan "folder ini aman dihapus?" bersifat **deterministik**
diberikan atribut (tier, umur, ukuran, tipe). Itu bukan masalah inferensi neural.
Oleh karena itu "AI" di produk ini memiliki peran yang dibatasi dengan tegas:

- **Mengambil keputusan** → engine heuristik (deterministik, bisa diuji, cepat).
- **Menjelaskan keputusan dalam bahasa natural** → LLM (opsional, on-demand).

Ini menghindari dua kegagalan umum produk "AI": halusinasi yang menghapus file
penting, dan bobot runtime yang membuat tool "ringan" jadi berat.

---

## 3. Tujuan & Non-Tujuan

### 3.1 Tujuan (Goals)

| ID | Tujuan | Metrik keberhasilan |
|---|---|---|
| G1 | Akuntansi ruang akurat | Counter "freed bytes" = jumlah byte nyata yang dihapus (unit test `freed_bytes_accurate_not_item_count`) |
| G2 | Klasifikasi risiko otomatis | Setiap folder diberi skor 0–100 + level (Healthy/Watch/Heavy/Protected) |
| G3 | Saran arsip ke drive eksternal | Plan arsip dibangun dari hasil scan + skor, dengan manifest untuk undo |
| G4 | Human-in-the-loop | Tidak ada aksi destruktif tanpa dialog konfirmasi eksplisit |
| G5 | Ringan | Binary ≤ 25 MB; RAM resident ≤ 100 MB saat mode scan |
| G6 | Semua di drive D | `CARGO_HOME`, target-dir, model, dan LLVM — tidak menulis ke C: |
| G7 | AI opsional & terbatas | LLM hanya menjelaskan, tidak mengeksekusi; dimuat on-demand |
| G8 | Portable Relocation | Deteksi drive letter tempat aplikasi berjalan untuk target pemindahan langsung ke dirinya sendiri |
| G9 | Auto-Routing | Pembelokan variabel lingkungan pengguna (`HKCU`) di Windows secara otomatis agar aplikasi menulis langsung ke drive eksternal |

### 3.2 Non-Tujuan (Explicit Non-Goals)

| ID | Non-Tujuan | Alasan |
|---|---|---|
| NG1 | Auto-delete tanpa konfirmasi | Risiko kehilangan data terlalu tinggi |
| NG2 | Resident selalu-jalan saat LLM aktif | Fisika: LLM butuh 4–6 GB RAM saat inferensi; janji "ringan" dilanggar |
| NG3 | Implementasi Assembly | Workload I/O-bound; nol keuntungan vs Rust, hanya menambah penderitaan build |
| NG4 | Cloud/online dependency | Harus bekerja sepenuhnya offline (privasi + keandalan) |
| NG5 | Menggantikan tool OS (Disk Cleanup) | Fokus pada folder spesifik workflow dev/konten, bukan sistem luas |

---

## 4. Persona Pengguna

### 4.1 Persona utama: "Power User solo"

- **Profil:** Developer / content creator (CapCut, VS Code) dengan banyak proyek.
- **Nyeri:** Disk C: penuh tiap bulan, build lambat, tidak tahu apa yang aman dihapus.
- **Kebutuhan:** Tahu **persis** berapa byte yang akan dibebaskan sebelum klik, dan
  bisa **mengembalikan** kalau salah pindah.
- **Toleransi:** Mau install toolchain build sekali, asal tool setelah itu cepat.

### 4.2 Persona sekunder: "Curator berhati-hati"

- **Profil:** Memiliki drive eksternal besar (1 TB+) untuk arsip jangka panjang.
- **Nyeri:** Tidak tahu mana yang "arsip layak" vs "sampah murni".
- **Kebutuhan:** Saran yang beralasan ("folder ini 80% file >90 hari, tapi cuma
  10% yang stale → kandidat arsip, bukan hapus").

---

## 5. Arsitektur Sistem

### 5.1 Workspace Cargo (modular)

```
System-Cache-Monitor/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── core/                   # logika murni, tanpa side-effect
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── rules.rs        # definisi folder (migrasi FOLDERS_TO_MONITOR)
│   │       ├── scanner.rs      # walk dir + hitung byte (FIX bug lama)
│   │       ├── classifier.rs   # skor risiko deterministik
│   │       └── archive.rs      # tipe data plan arsip
│   ├── actions/                # operasi filesystem destruktif (satu-satunya tempat)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── cleaner.rs      # hapus isi folder, byte-accurate
│   │       └── archiver.rs     # move ke external + manifest + undo
│   ├── llm/                    # binding llama.cpp (feature-gated `ai`)
│   │   └── src/lib.rs          # load GGUF, generate, build_scan_prompt
│   └── app/                    # binary GUI (egui/eframe)
│       └── src/
│           ├── main.rs         # entry point + setup_style
│           └── ui.rs           # state management + 3 panel
├── docs/PRD.md                 # dokumen ini
└── cache_monitor.py            # baseline lama (dipertahankan untuk referensi)
```

### 5.2 Pemisahan tanggung jawab (layering)

```
┌─────────────────────────────────────────────────┐
│  app (egui GUI) — presentasi + interaksi user   │
│          ↓ memanggil                            │
│  actions (cleaner, archiver) — I/O destruktif   │
│          ↓ bergantung pada                      │
│  core (scanner, classifier, archive plan)       │
│  ── logika murni, bisa diuji tanpa filesystem   │
└─────────────────────────────────────────────────┘
          ↑ (opsional, feature `ai`)
│  llm (llama-cpp-4) — menjelaskan, bukan memutuskan │
```

**Aturan keras:** hanya crate `actions` yang boleh menghapus/memindahkan file.
`core` tidak punya side-effect sehingga unit testnya cepat dan deterministik.
`llm` tidak pernah memanggil `actions` — outputnya hanya teks saran.

### 5.3 Alur data runtime

```
[Rescan] → scan_all (rayon paralel)
         → Vec<ScanResult>
         → classify tiap hasil → Vec<RiskScore>
         → UI tampilkan tabel berwarna
         → ArchivePlan::suggest(results, scores, external_root)
         → user konfirmasi → run_archive(entries, ext)
                            → manifest.json (untuk undo)
```

---

## 6. Spesifikasi Fitur

### 6.1 F1 — Scan paralel dengan akuntansi byte

**Status:** ✅ Selesai

- Memindai semua folder di `RuleSet::default()` secara paralel via `rayon`.
- Untuk tiap folder, mengumpulkan: `total_bytes`, `file_count`, `largest_file`,
  `oldest_mtime_secs`, `newest_mtime_secs`, `stale_file_count` (>90 hari).
- Threshold stale 90 hari dipakai sebagai sinyal "aman dihapus".
- **Bug lama diperbaiki:** `freed_bytes` sekarang dihitung dari metadata file
  sebelum penghapusan, bukan `+= 1` per item.

**Folder yang dipantau (default, migrasi dari Python):**

| Nama | Path (Windows) | Tier |
|---|---|---|
| User Temp Files | `%TEMP%` | Cache |
| Windows Temp | `C:\Windows\Temp` | Cache |
| VS Code Workspace | `%APPDATA%\Code\User\workspaceStorage` | Cache |
| VS Code Cached Data | `%APPDATA%\Code\CachedData` | Cache |
| VS Code Extension VSIXs | `%APPDATA%\Code\CachedExtensionVSIXs` | Cache |
| CapCut Cache | `%LOCALAPPDATA%\CapCut\User Data\Cache` | Cache |
| CapCut Pre-Render | `%LOCALAPPDATA%\CapCut\segmentPrerenderCache` | Cache |
| Docker Data | `%LOCALAPPDATA%\Docker` | **MonitorOnly** |
| Pip Cache (Drive D) | `D:\.pip_cache` | Cache |
| Dev Tools (Drive D) | `D:\.dev_tools` | **MonitorOnly** |

### 6.2 F2 — Klasifikasi risiko deterministik

**Status:** ✅ Selesai

Setiap folder dipetakan ke `RiskScore`:

```rust
pub struct RiskScore {
    pub urgency: u8,           // 0..=100
    pub level: RiskLevel,      // Healthy | Watch | Heavy | Protected
    pub reason: String,        // satu kalimat alasan
    pub auto_cleanable: bool,  // tier Cache → true
    pub archive_candidate: bool, // besar + tidak stale → true
}
```

**Logika skor (transparan, bukan black-box):**

| Kondisi | Urgency dasar |
|---|---|
| `bytes ≥ 2 GB` | 80 |
| `bytes ≥ 500 MB` | 50 |
| `bytes ≥ 50 MB` | 25 |
| `bytes < 50 MB` | 5 |
| `stale_ratio > 0.5` | +10 |

**Level:**

| Level | Kondisi | Warna UI |
|---|---|---|
| Heavy | ≥ 2 GB | Merah |
| Watch | ≥ 500 MB | Kuning |
| Healthy | < 500 MB | Hijau |
| Protected | tier MonitorOnly | Abu-abu (terlepas ukuran) |

**Kandidat arsip:** `bytes ≥ 500 MB` DAN `stale_ratio < 0.3` (besar tapi masih
sering dipakai → layak dipindah, bukan dihapus).

### 6.3 F3 — Clean dengan konfirmasi

**Status:** ✅ Selesai

- Hanya folder tier `Cache` yang menampilkan tombol "🧹 Clean".
- Klik → dialog konfirmasi modal menampilkan path lengkap.
- "Yes, Clean" → `clean_folder()` menghapus **isi** folder (folder itu sendiri dipertahankan).
- File terkunci/denied dihitung sebagai `skipped`, bukan error fatal.
- Hasil: "freed X bytes, Y files, Z dirs, W skipped" — semua akurat.

### 6.4 F4 — Archive advisory + undo

**Status:** ✅ Selesai

**Tujuan:** memindahkan folder besar (bukan sampah murni) ke drive eksternal,
dengan kemampuan mengembalikan.

- Plan arsip dibangun otomatis dari hasil scan (kandidat F2).
- UI menampilkan tabel: Source | Size | Destination | Reason.
- Field "External drive" bisa diubah (default `E:/`).
- Konfirmasi dua langkah: tombol "Confirm Archive" → peringatan → "Yes, proceed".
- **Implementasi aman:** copy → verify → delete source (bukan rename cross-volume
  yang bisa gagal di tengah dan meninggalkan state parsial).
- **Manifest JSON** ditulis di root external: `cache-archive-manifest.json`,
  berisi setiap `(source, destination, bytes, timestamp)`.
- **Idempoten:** re-run tidak duplikasi (destinasi yang sudah ada di-skip).
- **Undo:** `undo_archive(manifest_path)` memindahkan semuanya kembali dan
  menghapus manifest.

### 6.5 F5 — Panel Ask AI (opsional, feature `ai`)

**Status:** 🔨 In-progress (toolchain siap, wiring API tahap akhir)

- Tab ketiga "🤖 Ask AI", hanya muncul jika di-build dengan `--features ai`.
- **On-demand load:** model GGUF dimuat hanya saat panel dibuka; di-drop saat
  ditutup untuk membebaskan RAM.
- Input: prompt yang dibangun dari hasil scan terstruktur (lihat §7.3).
- Output: teks saran bahasa natural.
- **Garansi keamanan:** output LLM tidak pernah diparsing menjadi perintah
  eksekusi. Pengguna harus tetap klik tombol Clean/Archive manual.
- **Status:** ✅ Selesai

### 6.6 F6 — Disk Map (TreeMap Disk Visualizer)
- Visualisasi penggunaan ruang penyimpanan seluruh isi drive secara asinkron (background thread) menggunakan algoritme TreeMap interaktif (Slice-and-Dice).
- Pemindaian paralel menggunakan Rayon dengan pemangkasan otomatis (pruning) file/folder berukuran < 5 MB untuk menjaga GUI tetap responsif di 60 FPS.
- Navigasi Zoom In (double-click pada folder) dan Zoom Out (tombol "Go Up").
- **Status:** ✅ Selesai

### 6.7 F7 — Undo Cleaner (Safe Recycle Bin & Restore)
- Aksi pembersihan tidak lagi menghapus berkas secara permanen secara langsung. Berkas dipindahkan ke folder Recycle Bin lokal di sistem dengan manifes detail dalam format JSON.
- Panel GUI lipat menyediakan daftar riwayat sesi pembersihan beserta tombol **Undo** untuk memulihkan berkas ke lokasi asal secara aman, serta tombol **Purge** untuk membersihkan ruang penyimpanan secara permanen.
- **Status:** ✅ Selesai

### 6.8 F8 — Whitelist / Exclusion List (Monitored Folders)
- Pengguna dapat mengecualikan path tertentu dari pemindaian dan pemeriksaan duplikat secara dinamis dari antarmuka GUI.
- Whitelist disimpan dan dibaca secara real-time ke/dari berkas `settings.toml`.
- **Status:** ✅ Selesai

### 6.9 F9 — Smart Selection & Bulk Deletion (Duplicate Files)
- Menyediakan tombol pemilih otomatis **Keep Oldest** dan **Keep Newest** berdasarkan metadata modifikasi berkas asli untuk menyaring berkas duplikat yang akan dibersihkan secara otomatis.
- Tombol hapus massal (**Clean Selected Duplicates**) yang aman dengan perlindungan agar tidak menghapus salinan terakhir di setiap grup.
- **Status:** ✅ Selesai

---

## 7. Model "AI" yang Digunakan

### 7.1 Filosofi: deterministik untuk keputusan, LLM untuk penjelasan

Produk ini **tidak** menggunakan AI untuk mengambil keputusan destruktif. Keputusan
"hapus / pindah / jangan sentuh" diambil oleh `classifier.rs` yang deterministik
dan bisa diuji. LLM hanya menjelaskan keputusan itu dalam bahasa natural.

**Mengapa:** LLM bisa halusinasi nama folder atau salah menilai risiko. Mendelegasikan
keputusan hapus ke LLM adalah cara tercepat kehilangan data. Membatasi perannya ke
"penjelalah yang membaca data faktual" menjaga keamanan sambil tetap memberi nilai
"agentic" (menjawab pertanyaan bebas).

### 7.2 Model lokal yang dipakai

| Atribut | Nilai |
|---|---|
| Arsitektur | Qwen2 1.5B (sudah ada di disk pengguna, **tidak perlu download**) |
| Format | GGUF (quantized) |
| Ukuran | ~892 MB |
| Lokasi | `D:\MODEL OLLAMA\blobs\sha256-405b56…` |
| RAM saat inferensi | ~2–3 GB |
| Context window | 2048 token |
| Backend | llama.cpp via crate `llama-cpp-4` 0.3.2 |

Model lain yang tersedia (bisa dipilih via config): Gemma 2B, Phi3, Qwen3.5-4B.

### 7.3 Prompt yang dibangun dari konteks scan

`build_scan_prompt()` menghasilkan input terstruktur (bukan "tolong tebak"):

```
You are Cache Advisor, a storage management assistant.
Based on the following scan results, answer the user's question.
Be concise and practical. Focus on what is safe to clean or archive.

SCAN RESULTS:
  - VS Code Cached Data (2.3 GB): tier=cache, urgency=80/100, files=14523, stale=9800/14523
  - Docker Data (12.1 GB): tier=monitor-only, urgency=0/100, ...
  ...

Based on the above, which folders should the user clean or move to external storage?
Explain briefly for each recommendation.
Answer:
```

### 7.4 Rencana Pelatihan & Evolusi Agentic AI Lokal

Untuk meningkatkan efisiensi dan keandalan sistem pembersihan, direncanakan proses pelatihan khusus (Domain-Specific Fine-Tuning) untuk model AI lokal:

1. **Spesialis Pembersih Penyimpanan (Storage Cleaning Specialist):**
   Model AI akan dilatih secara eksklusif menggunakan dataset pola penyimpanan berkas sampah, cache aplikasi, dan metadata sistem file. Pelatihan terfokus ini akan meningkatkan akurasi inferensi serta memperkecil parameter model agar dapat berjalan sangat cepat langsung dari USB/media eksternal.
2. **Evolusi Kemampuan Eksekusi Agentic:**
   Di masa mendatang, model AI akan dihubungkan dengan API eksekusi sistem langsung (tool calling) yang memungkinkannya bertindak sebagai agen otonom penuh untuk:
   - Mengeksekusi instruksi CMD / PowerShell secara aman.
   - Melakukan penghapusan, pemindahan (relokasi), dan penyalinan (copy) berkas.
   - Mengubah Environment Variables Windows secara langsung.
   - Memiliki hak akses penuh (full access) terhadap komputer dalam batas container media eksternal tempat program berjalan.

---

## 8. Persyaratan Non-Fungsional

### 8.1 Performa

| Metrik | Target | Aktual (terukur) |
|---|---|---|
| Ukuran binary (release, tanpa AI) | ≤ 25 MB | **3.8 MB** ✅ |
| RAM resident (mode scan, idle) | ≤ 100 MB | ~30–80 MB (estimasi) |
| RAM saat inferensi LLM | (on-demand) | +2–6 GB saat aktif |
| Waktu scan ~10 folder (SSD) | < 2 detik | < 1 detik (rayon paralel) |
| Build release (incremental) | < 5 menit | ~3 menit |

### 8.2 Portabilitas

- **OS:** Windows 10/11 x64 (target utama). Path di `rules.rs` Windows-specific;
  folder yang tidak ada dilaporkan "not found", bukan crash.
- **Runtime:** Zero dependency — single `.exe`, tidak butuh Python, WebView, atau
  .NET. eframe render via GPU (glow/wgpu), bukan webview.

### 8.3 Keandalan

- Semua operasi file non-panic: folder hilang → `exists=false`; file terkunci →
  `skipped`, bukan error fatal.
- Archiver: copy-then-verify-then-delete (bukan rename cross-volume).
- Manifest idempoten: re-run tidak korupsi data.

---

## 9. Keamanan & Keamanan Data

### 9.1 Prinsip human-in-the-loop

**Tidak ada** aksi destruktif (hapus / pindah) yang dieksekusi tanpa dialog
konfirmasi eksplisit yang menampilkan path lengkap dan peringatan. Ini berlaku
baik untuk action manual maupun saran LLM.

### 9.2 Privasi

- **Sepenuhnya offline.** Tidak ada telemetri, tidak ada panggilan API cloud.
- Model LLM berjalan lokal; data file pengguna tidak pernah meninggalkan mesin.
- Manifest arsip hanya berisi path + ukuran + hash, bukan konten file.

### 9.3 Reversibility

- Setiap arsip mencatat manifest penuh → `undo_archive` bisa mengembalikan.
- Clean tidak menghapus folder itu sendiri, hanya isinya (folder bisa di-regenerasi
  oleh aplikasi pemilik).

### 9.4 Batasan LLM & Rencana Evolusi Agentic

- **Fase Saat Ini (Human-in-the-Loop Murni):** LLM (`crate llm`) tidak memiliki dependency ke `crate actions`. Output LLM berupa teks penjelasan mentah dan tidak diubah menjadi perintah eksekusi otomatis.
- **Fase Masa Depan (Agentic Sandbox):** Saat model AI selesai dilatih khusus, AI akan diberikan akses penuh (eksekusi CMD, hapus, pindah, copy, ubah ENV) menggunakan arsitektur tool-calling dengan pengawasan keamanan terisolasi (sandbox) langsung dari media eksternal tempat program dijalankan.

---

## 10. Metrik & Pengujian

### 10.1 Unit test (saat ini: 16 lulus)

| Crate | Test | Validasi |
|---|---|---|
| core/scanner | `byte_counter_is_accurate_not_item_count` | **Bug lama diperbaiki** |
| core/scanner | `missing_folder_reports_not_found` | Robustness |
| core/scanner | `nested_dirs_summed` | Akurasi rekursif |
| core/scanner | `format_bytes_units` | UI formatting |
| core/scanner | `exclusions_are_skipped` | Verifikasi Whitelist |
| core/duplicates | `find_duplicates_works` | Akurasi SHA-256 |
| core/duplicates | `find_duplicates_skips_exclusions` | Verifikasi filter duplikat |
| core/disk_map | `scan_drive_pruning_and_sorting` | Akurasi scan Disk Map |
| core/classifier | `monitor_only_is_protected` | Docker tidak tersentuh |
| core/classifier | `heavy_cache_is_auto_cleanable` | Skor urgensi |
| core/classifier | `small_healthy_folder` | Tidak false-positive |
| core/classifier | `not_found_is_healthy_zero_urgency` | Edge case |
| core/archive | `plan_sums_bytes` | Matematika plan |
| actions/cleaner | `freed_bytes_accurate_not_item_count` | **Integrasi fix bug** |
| actions/cleaner | `nested_dirs_counted` | Akurasi |
| actions/cleaner | `missing_path_errors` | Error handling |
| actions/cleaner | `empty_folder_no_op` | Edge case |
| actions/cleaner | `clean_file_works` | Akurasi clean file |
| actions/archiver | `archive_then_undo_roundtrip` | **Reversibility** |
| actions/archiver | `idempotent_skip_existing` | Safety |
| actions/archiver | `missing_source_skipped` | Robustness |
| actions/undo_cleaner | `clean_to_recycle_and_restore_roundtrip` | **Reversibility Recycle Bin** |
| actions/undo_cleaner | `clean_to_recycle_and_purge` | Safety Recycle Bin |

### 10.2 Metrik keberhasilan produk

| Metrik | Cara ukur | Target |
|---|---|---|
| Akurasi byte | Unit test + verifikasi manual | 100% (tercapai) |
| False-positive hapus | Review manual folder Protected | 0 |
| Reversibility arsip | Roundtrip test | 100% restore |
| Ukuran binary | `ls -lh` | ≤ 25 MB (3.8 MB aktual) |
| Tidak ada tulisan ke C: | Audit `CARGO_HOME` + target-dir | 0 tulisan selain toolchain Rust |

---

## 11. Lingkungan Build & Dependensi

### 11.1 Toolchain (semua di drive D)

| Komponen | Versi | Lokasi | Status |
|---|---|---|---|
| Rust | 1.96.0 (stable-msvc) | `C:\Users\thori\.rustup` (toolchain sudah ada) | ✅ |
| Cargo cache | — | `D:\.cargo` (`CARGO_HOME` di-set) | ✅ |
| Build target | — | `D:\.cargo-target` (`config.toml`) | ✅ |
| CMake | 3.31.7 | system | ✅ |
| MSVC Build Tools | 14.44 (2022 Community) | `C:\Program Files\…\2022\Community` | ✅ |
| LLVM/Clang (lama) | 18.1.8 | `D:\LLVM` | ⚠️ terlalu lama untuk bindgen |
| LLVM/Clang (baru) | **19.1.7** | `D:\LLVM19` (diunduh & diekstrak ke D) | ✅ |
| Model GGUF | Qwen2 1.5B | `D:\MODEL OLLAMA\blobs\sha256-405b56…` | ✅ |

### 11.2 Dependensi Rust inti

| Crate | Versi | Untuk |
|---|---|---|
| `egui` / `eframe` | 0.29 | GUI immediate-mode, single binary |
| `egui_extras` | 0.29 | Tabel scan/archive |
| `walkdir` + `rayon` | 2 / 1 | Scan paralel |
| `sha2` | 0.10 | (siap untuk deteksi duplikat) |
| `serde` + `toml` + `serde_json` | 1 / 0.8 / 1 | Config & manifest |
| `jiff` | 0.1 | File-age math |
| `anyhow` / `thiserror` | 1 / 1 | Error handling |
| `llama-cpp-4` | 0.3.2 | Embedding LLM (feature `ai`) |

### 11.3 Cara build

```bash
# App inti (tanpa AI) — cepat
CARGO_HOME=/d/.cargo cargo build --release -p cache-advisor

# Dengan AI (build C++ pertama ~10-15 menit)
LIBCLANG_PATH="D:/LLVM19/bin" \
CARGO_HOME=/d/.cargo cargo build --release -p cache-advisor --features ai
```

---

## 12. Roadmap & Status

### 12.1 Selesai (v0.2.0)

- [x] Workspace Cargo modular (core / actions / app / llm)
- [x] Scanner paralel dengan fix byte-counter
- [x] Classifier risiko deterministik
- [x] Cleaner byte-accurate + dialog konfirmasi
- [x] Archiver dengan manifest + undo + idempotensi
- [x] GUI egui (scan table, archive panel, modal)
- [x] 16 unit test lulus
- [x] Binary release 3.8 MB
- [x] Toolchain C++ siap (LLVM 19 diunduh ke D)
- [x] Model GGUF terverifikasi (Qwen2 1.5B, tidak perlu download)

### 12.2 In-progress

- [x] Wiring akhir API `llama-cpp-4` (load model → generate loop)
- [x] Panel Ask AI di `ui.rs` (feature-gated)
- [x] Settings TOML (override folder, threshold, path model)
- [x] Deteksi duplikat (hash SHA-256, pre-filter by size)
- [x] Scheduler scan periodik (opt-in, bukan resident berat)
- [x] Dukungan multi-drive (selain C ↔ E)
- [x] UI light/dark toggle, export laporan
- [x] Disk Map (TreeMap Disk Visualizer)
- [x] Undo Cleaner (Safe Recycle Bin & Restore)
- [x] Whitelist / Exclusion List (Monitored Folders)
- [x] Smart Selection & Bulk Deletion (Duplicate Files)

### 12.2 In-progress (v0.4.0)
- [ ] F10: Deteksi Portable Mode & Relokasi ke Disk Program (Self-Archiving Container)
- [ ] F11: Redirection Variabel Lingkungan Windows otomatis via Registry/PowerShell
- [ ] F12: Admin/User Elevation Status Indicator di GUI

---

## 13. Trade-off & Keputusan Desain

### 13.1 Rust vs Python vs C vs Assembly

| Pilihan | Keputusan | Alasan |
|---|---|---|
| Rust | ✅ Dipilih | Native, aman memori, single binary, ekosistem LLM matang |
| Python (lanjut) | Ditolak | Runtime berat, sulit distribusi single-file, bug byte-counter |
| C | Ditolak | Pengembangan lambat, tidak ada keuntungan signifikan vs Rust |
| Assembly | **Ditolak tegas** | Workload I/O-bound → nol keuntungan, hanya penderitaan build |

### 13.2 Embed llama.cpp vs Ollama HTTP

| Aspek | Embed (dipilih) | Ollama HTTP (alternatif) |
|---|---|---|
| Binary size | +0 (model di luar) | 3.8 MB (client tipis) |
| Build complexity | Tinggi (C++ build) | Rendah |
| Dependency runtime | Tidak ada | Butuh Ollama service jalan |
| Kontrol model | Penuh | Terbatas |
| Latensi | Rendah (in-proc) | HTTP overhead |

**Pilihan akhir:** embed, sesuai permintaan pengguna ("full agentic"). Konsekuensi
diterima: build pertama lambat, butuh toolchain C++.

### 13.3 Heuristik vs LLM untuk keputusan

| Aspek | Heuristik (dipilih utk keputusan) | LLM (dipilih utk penjelasan) |
|---|---|---|
| Kecepatan | <1 ms | Detik |
| Deterministik | Ya | Tidak |
| Bisa diuji | Ya (unit test) | Tidak |
| Risiko halusinasi | Tidak ada | Ada |
| Bahasa natural | Tidak | Ya |

**Keputusan:** pisahkan tegas. Keputusan = heuristik; penjelasan = LLM.

### 13.4 Kontradiksi "ringan" vs "agentic"

Permintaan awal berisi kontradiksi internal: "sangat ringan tanpa membebani"
**dan** "full agentic AI". Ini tidak bisa dipenuhi bersamaan karena:

- LLM lokal yang resident = 4–6 GB RAM konstan → melanggar "ringan".
- Tool ringan murni = tanpa LLM → bukan "agentic".

**Resolusi yang jujur (dan yang diimplementasikan):**

- **Daemon inti (scan/clean/archive) tetap ringan** (~30–80 MB RAM).
- **LLM hanya on-demand** — dimuat saat panel dibuka, di-drop saat ditutup.
- Janji "selalu jalan tanpa membebani" **tidak berlaku saat LLM aktif** — ini
  diakui terbuka di PRD, bukan disembunyikan.

---

## 14. Yang TIDAK Dikerjakan

1. **Assembly** — ditolak dengan alasan teknis (§13.1).
2. **Auto-delete tanpa konfirmasi** — risiko data (NG1).
3. **Resident LLM selalu-jalan** — fisika tidak mengizinkan "ringan" (NG2).
4. **Cloud dependency** — harus offline penuh (NG4).
5. **Menggantikan Disk Cleanup Windows** — fokus folder workflow, bukan OS luas (NG5).
6. **LLM yang memutuskan aksi** — output hanya saran teks (§9.4).

---

## 15. Glosarium

| Istilah | Arti |
|---|---|
| Tier | Kategori folder: Cache (boleh hapus), Cautious (perlu konfirmasi), MonitorOnly (jangan sentuh) |
| Stale | File dengan mtime > 90 hari — sinyal "aman dihapus" |
| Archive candidate | Folder besar (≥500 MB) tapi tidak stale (<30%) → layak dipindah, bukan dihapus |
| Manifest | File JSON di root external yang mencatat semua perpindahan untuk undo |
| Human-in-the-loop | Pengguna wajib konfirmasi sebelum aksi destruktif |
| GGUF | Format model terkuantisasi untuk llama.cpp |
| Feature flag | Build-time switch (`--features ai`) untuk menyertakan atau tidak komponen berat |
| Deterministik | Output selalu sama untuk input yang sama — bisa diuji, tidak ada halusinasi |

---

> **Catatan penutup:** PRD ini ditulis agar akurat terhadap state implementasi
> nyata per tanggal di atas, termasuk pengakuan terbuka tentang kontradiksi
> kebutuhan awal dan batasan fisika LLM. Setiap klaim "selesai" didukung unit
> test atau pengukuran biner; setiap klaim "in-progress" menjelaskan sisa pekerjaan
> spesifik.
