-- Field dari desain Stitch yang belum ada di skema awal.
-- kode_proyek: kode internal seperti PKM-JB-GRT-042 (opsional).
-- target_sasaran: jumlah KK sasaran proyek. 0 untuk proyek lama yang belum diisi.
ALTER TABLE projects
    ADD COLUMN kode_proyek VARCHAR(64) NULL,
    ADD COLUMN target_sasaran INT UNSIGNED NOT NULL DEFAULT 0;

-- NIK 16 digit per pemanfaat (opsional). Di PDF hanya ditampilkan tersamar.
ALTER TABLE beneficiaries
    ADD COLUMN nik CHAR(16) NULL;

-- Koordinat GPS saat foto diunggah (opsional, dari perangkat).
ALTER TABLE photos
    ADD COLUMN latitude DOUBLE NULL,
    ADD COLUMN longitude DOUBLE NULL;
