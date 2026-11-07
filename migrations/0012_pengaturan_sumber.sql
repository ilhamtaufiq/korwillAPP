-- Pengaturan sumber data Lapming: link Google Sheets Sanimas dan LPK, serta tahun anggaran pekerjaan.
-- Diubah admin lewat halaman Pengaturan. Tidak perlu lagi copy-paste link setiap impor.
CREATE TABLE lapming_pengaturan (
    kunci  VARCHAR(64) NOT NULL PRIMARY KEY,
    nilai  TEXT NOT NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

-- Riwayat setiap kali data diperbarui dari link (per jenis: sanimas atau lpk).
CREATE TABLE lapming_update_log (
    id             BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    waktu          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    pengguna       VARCHAR(64) NOT NULL,
    jenis          ENUM('san', 'lpk') NOT NULL,
    status         VARCHAR(16) NOT NULL,
    baris          INT NOT NULL DEFAULT 0,
    perubahan      INT NOT NULL DEFAULT 0,
    pekerjaan_baru INT NOT NULL DEFAULT 0,
    catatan        TEXT NULL,
    KEY idx_lapming_update_waktu (waktu)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

INSERT INTO lapming_pengaturan (kunci, nilai) VALUES
    ('link_san', 'https://docs.google.com/spreadsheets/d/1DekLV8Vobz1Y-ctymL-5JLOIwNE8u01I12IB7g0bvIo/edit?gid=267727772#gid=267727772'),
    ('link_lpk', 'https://docs.google.com/spreadsheets/d/11Q46nIuPOeKSPDUG1jcJbx2PPcJm5i7v2jLUIehdrUI/edit?gid=1088104240#gid=1088104240'),
    ('tahun_anggaran', '2026');
