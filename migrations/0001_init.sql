-- Skema awal aplikasi dokumentasi konstruksi Sanimas (MySQL 8.0.16+ untuk CHECK).
-- Jenis bangunan memakai ENUM karena template LPK sama dengan Sanimas (Bilik dan Tangki Septik),
-- sehingga tabel building_types belum diperlukan.

CREATE TABLE users (
    id            BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    username      VARCHAR(64)  NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role          ENUM('admin', 'tfl') NOT NULL,
    created_at    TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_users_username (username)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE TABLE projects (
    id         BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    judul      VARCHAR(255) NOT NULL,
    kegiatan   VARCHAR(255) NOT NULL,
    pokmas     VARCHAR(255) NOT NULL,
    desa       VARCHAR(128) NOT NULL,
    kecamatan  VARCHAR(128) NOT NULL,
    kabupaten  VARCHAR(128) NOT NULL,
    tahun      SMALLINT UNSIGNED NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_projects_lokasi (kabupaten, kecamatan, desa)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE TABLE beneficiaries (
    id           BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    project_id   BIGINT UNSIGNED NOT NULL,
    no           INT UNSIGNED NOT NULL,
    nama         VARCHAR(255) NOT NULL,
    rt_rw_dusun  VARCHAR(255) NOT NULL DEFAULT '',
    UNIQUE KEY uq_beneficiaries_no (project_id, no),
    CONSTRAINT fk_beneficiaries_project FOREIGN KEY (project_id) REFERENCES projects (id) ON DELETE CASCADE
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

-- Satu sel tabel PDF = satu baris. UNIQUE membuat upload ulang menimpa foto lama.
CREATE TABLE photos (
    id             BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    beneficiary_id BIGINT UNSIGNED NOT NULL,
    building       ENUM('bilik', 'tangki_septik') NOT NULL,
    stage          TINYINT UNSIGNED NOT NULL,
    path           VARCHAR(512) NOT NULL,
    thumb_path     VARCHAR(512) NOT NULL,
    taken_at       DATETIME NULL,
    uploaded_by    BIGINT UNSIGNED NULL,
    created_at     TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_photos_cell (beneficiary_id, building, stage),
    CONSTRAINT chk_photos_stage CHECK (stage IN (0, 25, 50, 75, 100)),
    CONSTRAINT fk_photos_beneficiary FOREIGN KEY (beneficiary_id) REFERENCES beneficiaries (id) ON DELETE CASCADE,
    CONSTRAINT fk_photos_user        FOREIGN KEY (uploaded_by)    REFERENCES users (id)         ON DELETE SET NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;
