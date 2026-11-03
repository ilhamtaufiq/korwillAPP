-- Data laporan mingguan Lapming (Sanimas dan LPK). Setiap baris Pokmas per sheet minggu disimpan utuh
-- sebagai JSON supaya laporan bisa dibuat ulang tanpa file sumber, dan perubahan antar pemuatan bisa dibandingkan.
CREATE TABLE lapming_data (
    id         BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    jenis      ENUM('san', 'lpk') NOT NULL,
    sheet      VARCHAR(16) NOT NULL,
    no_key     VARCHAR(64) NOT NULL,
    korfas     VARCHAR(128) NOT NULL,
    data       LONGTEXT NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    KEY idx_lapming_data (jenis, sheet, no_key),
    KEY idx_lapming_korfas (korfas)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

-- Nama sheet minggu (M1, M2, ...) yang ada di file sumber, termasuk yang header-nya tidak terbaca.
CREATE TABLE lapming_sheet (
    jenis ENUM('san', 'lpk') NOT NULL,
    nama  VARCHAR(16) NOT NULL,
    PRIMARY KEY (jenis, nama)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

-- Riwayat perubahan nilai antar pemuatan file sumber.
CREATE TABLE lapming_log (
    id      BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    waktu   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    jenis   ENUM('san', 'lpk') NOT NULL,
    sheet   VARCHAR(16) NOT NULL,
    no_key  VARCHAR(64) NOT NULL,
    pokmas  VARCHAR(255) NOT NULL,
    korfas  VARCHAR(128) NOT NULL,
    ubah    LONGTEXT NOT NULL,
    KEY idx_lapming_log_korfas (korfas, waktu)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

-- Nama Pokmas di sumber yang berbeda ejaan dengan nama baku. nama_norm = huruf kecil tanpa awalan Pokmas.
CREATE TABLE lapming_alias (
    nama_norm   VARCHAR(255) NOT NULL PRIMARY KEY,
    nama_sumber VARCHAR(255) NOT NULL,
    nama_baku   VARCHAR(255) NOT NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

INSERT INTO lapming_alias (nama_norm, nama_sumber, nama_baku) VALUES
    ('tunasmandiri', 'Tunas Mandiri', 'Taruna Mandiri'),
    ('sauyunanciapyunggirang', 'Sauyunan Ciapyung Girang', 'Sauyunan Cipayung Girang'),
    ('karyamudamandiri', 'Karya Muda mandiri', 'Karya Muda Mandiri');

-- RPDB per Pokmas dan tahap. Tahap bertanggal setelah tanggal laporan tidak dihitung; jumlah kosong = belum diisi.
CREATE TABLE lapming_rpdb (
    nama_norm VARCHAR(255) NOT NULL,
    nama_baku VARCHAR(255) NOT NULL,
    tahap_no  TINYINT UNSIGNED NOT NULL,
    jumlah    BIGINT NULL,
    tanggal   DATE NULL,
    PRIMARY KEY (nama_norm, tahap_no)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;