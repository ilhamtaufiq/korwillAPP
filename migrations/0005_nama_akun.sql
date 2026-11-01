-- Nama lengkap akun, dipakai untuk menghubungkan TFL ke pekerjaan dari kolom TFL di rekap Lapming.
ALTER TABLE users
    ADD COLUMN nama VARCHAR(255) NULL;
