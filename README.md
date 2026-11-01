# KorwilAPP

Aplikasi dokumentasi konstruksi Sanimas untuk Korwil dan TFL. Pekerjaan dan pemanfaat dicatat per pokmas, foto progres diunggah per sel (Bilik dan Tangki Septik, tahap 0% sampai 100%), dan hasilnya diekspor ke PDF A4 landscape.

Domain produksi: **https://korwil.cianjur.space**

## Fitur

- Login dengan username dan kata sandi, peran admin, korwil, dan TFL.
- Daftar dan detail pekerjaan, daftar pemanfaat (dengan NIK opsional), dan progres foto.
- Unggah foto dari kamera atau galeri, dengan pratinjau. Foto disimpan utuh dan diperkecil jika terlalu besar.
- Ekspor PDF dengan opsi sel kosong dan catatan tambahan, plus pratinjau langsung.
- Impor data awal dari rekap Lapming (.xlsx). Akun korwil dan TFL dibuat otomatis.
- Dashboard ringkasan dan daftar user (khusus admin).
- Log Dokumentasi (khusus admin): riwayat tambah, ubah, dan hapus pekerjaan, pemanfaat, dan foto, lengkap dengan siapa dan kapan.
- Hapus lunak: data yang dihapus tidak langsung hilang dan bisa dipulihkan dari Log Dokumentasi. Foto yang dihapus tetap tersimpan di disk.

## Teknologi

- Backend: Rust, Axum, SQLx (MySQL 8), printpdf, calamine (baca xlsx).
- Frontend: HTML statis dengan Tailwind CSS dan JavaScript biasa, disajikan oleh Axum.
- Database: MySQL 8. Migrasi dijalankan otomatis saat aplikasi start.

## Struktur

```
src/                 kode backend (auth, projects, photos, export, import, users, dashboard)
migrations/          skema MySQL (sqlx), dijalankan otomatis
static/              halaman dan script frontend
Dockerfile           build image produksi
docker-compose.yml   aplikasi + MySQL, dipakai Coolify
```

## Menjalankan di lokal

Butuh Rust (stable) dan MySQL 8.

1. Buat database kosong:

   ```bash
   mysql -uroot -e "CREATE DATABASE IF NOT EXISTS sanimas_dev CHARACTER SET utf8mb4;"
   ```

2. Jalankan dari folder root proyek. Migrasi dijalankan otomatis. Default `DATABASE_URL` adalah `mysql://root@127.0.0.1:3306/sanimas_dev`.

   ```bash
   cargo run
   ```

3. Buka `http://127.0.0.1:8080`.

Aplikasi harus dijalankan dari folder root proyek, karena folder `static/` dan `uploads/` dibaca dari lokasi itu.

## Mengubah tampilan (CSS)

Gaya dibuat dengan Tailwind yang di-build sekali, hasilnya ada di `static/css/app.css` dan ikut di repository. Setelah mengubah kelas Tailwind atau warna di halaman, jalankan:

```bash
npm install
npm run build:css
```

Warna dan font ada di `tailwind.config.js`. Untuk pengembangan, `npm run watch:css` membangun ulang setiap ada perubahan.

## Membuat akun admin pertama

Aplikasi tidak membuat akun admin otomatis. Buat akun lewat perintah `create-user`. Password dibaca dari stdin supaya tidak tersimpan di riwayat shell:

```bash
echo 'password-yang-kuat' | cargo run -- create-user nama_admin admin
```

Peran yang tersedia: `admin`, `korwil`, `tfl`. Di Docker:

```bash
echo 'password-yang-kuat' | docker compose exec -T app sanimas-dokumentasi create-user nama_admin admin
```

## Deploy ke Coolify

1. Di Coolify, buat **New Resource → Docker Compose**, lalu hubungkan repository ini (branch `main`).
2. Isi environment variable (menu *Environment Variables*):
   - `DB_PASSWORD`: password user aplikasi MySQL.
   - `DB_ROOT_PASSWORD`: password root MySQL.

   Lihat `.env.example` untuk contoh. Password dengan karakter khusus (`@ : / ? # %`) harus di-URL-encode, karena dipakai di `DATABASE_URL`.
3. Di layanan **app**, isi domain `korwil.cianjur.space` dengan port `8080`. Coolify memasang HTTPS otomatis, jadi pastikan DNS domain sudah diarahkan ke server Coolify.
4. Deploy. Setelah container `app` jalan, buat akun admin pertama lewat terminal container `app` di Coolify:

   ```bash
   echo 'password-yang-kuat' | sanimas-dokumentasi create-user nama_admin admin
   ```

5. Login di `https://korwil.cianjur.space`.

Data disimpan di dua volume Docker: `dbdata` (database MySQL) dan `uploads` (foto). Keduanya tetap ada saat deploy ulang.

## Variabel environment

| Variabel | Default | Keterangan |
| --- | --- | --- |
| `DATABASE_URL` | `mysql://root@127.0.0.1:3306/sanimas_dev` | URL koneksi MySQL. Di compose sudah diisi otomatis dari `DB_PASSWORD`. |
| `BIND` | `127.0.0.1:8080` | Alamat dan port server. Di Docker diset ke `0.0.0.0:8080`. |
| `DB_PASSWORD` | - | Password user `korwil` (hanya untuk compose). |
| `DB_ROOT_PASSWORD` | - | Password root MySQL (hanya untuk compose). |

## Backup

- Database: `docker compose exec db mysqldump -uroot -p sanimas > backup.sql`
- Foto: salin isi volume `uploads`, misalnya dengan `docker run --rm -v <nama_volume>:/data -v $PWD:/backup alpine tar czf /backup/uploads.tgz -C /data .`

## Catatan keamanan

- Ganti password admin awal dan jangan memakai password yang mudah ditebak.
- Impor Lapming membuat akun korwil dan TFL dengan **password sama dengan username**, dan akun itu ditandai wajib ganti kata sandi. Peringatan ditampilkan di aplikasi, tetapi tidak memblokir login. Dorong user untuk segera mengganti kata sandi.
- Sesi disimpan di memori server. Setelah server restart, user harus login ulang.
- Jangan commit file `.env` yang berisi password asli. File ini sudah masuk `.gitignore`.
