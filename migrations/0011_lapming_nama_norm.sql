-- Nama Pokmas yang sudah dinormalisasi (huruf kecil, tanpa awalan dan spasi) di setiap baris data Lapming.
-- Dipakai untuk mencari baris satu Pokmas tanpa membaca seluruh data. Baris lama diisi saat pertama dibaca
-- (lihat backfill di api.rs); nilai '' berarti belum diisi.
ALTER TABLE lapming_data
    ADD COLUMN nama_norm VARCHAR(255) NOT NULL DEFAULT '',
    ADD KEY idx_lapming_nama (nama_norm);
