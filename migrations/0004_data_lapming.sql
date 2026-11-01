-- Data awal dari rekap Lapming (impor Excel). Semua opsional.
-- korwil_nama: KORFAS / Korwil. tfl_nama: TFL (kolom Teknik). pagu_dana: pagu dana dalam rupiah.
ALTER TABLE projects
    ADD COLUMN korwil_nama VARCHAR(255) NULL,
    ADD COLUMN tfl_nama VARCHAR(255) NULL,
    ADD COLUMN pagu_dana BIGINT UNSIGNED NULL;
