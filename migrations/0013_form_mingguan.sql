-- Data isian Form Laporan Mingguan yang belum ada di Lapming: tanggal tahapan, target per lokasi,
-- partisipan masyarakat, dan permasalahan. Lokasi dikenali dari kunci teks (lihat laporan_mingguan::kunci_lokasi).
CREATE TABLE form_lokasi_data (
    korwil     VARCHAR(255) NOT NULL,
    jenis      ENUM('san', 'lpk') NOT NULL,
    lokasi     VARCHAR(512) NOT NULL,
    tahapan    LONGTEXT NOT NULL,
    target     LONGTEXT NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (korwil, jenis, lokasi)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE TABLE form_baris (
    korwil  VARCHAR(255) NOT NULL,
    bagian  ENUM('partisipan', 'permasalahan') NOT NULL,
    urut    INT UNSIGNED NOT NULL,
    data    LONGTEXT NOT NULL,
    PRIMARY KEY (korwil, bagian, urut)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;
