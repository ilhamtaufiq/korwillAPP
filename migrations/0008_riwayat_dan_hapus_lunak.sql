-- Hapus lunak: data yang dihapus hanya diberi tanda waktu deleted_at, lalu bisa dipulihkan dari riwayat.
-- Foto yang dihapus tetap ada di disk supaya bisa dikembalikan.
ALTER TABLE projects
    ADD COLUMN deleted_at DATETIME NULL,
    ADD INDEX idx_projects_deleted (deleted_at);

ALTER TABLE beneficiaries
    ADD COLUMN deleted_at DATETIME NULL,
    ADD INDEX idx_beneficiaries_deleted (deleted_at);

ALTER TABLE photos
    ADD COLUMN deleted_at DATETIME NULL,
    ADD INDEX idx_photos_deleted (deleted_at);

-- Riwayat setiap perubahan data dokumentasi: create, update, delete, dan restore.
-- project_id disimpan supaya riwayat bisa difilter per pekerjaan. Tidak memakai foreign key
-- ke tabel data supaya riwayat tetap ada walau data sumbernya nanti dihapus permanen.
CREATE TABLE log_dokumentasi (
    id          BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    waktu       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id     BIGINT UNSIGNED NULL,
    username    VARCHAR(64) NOT NULL,
    aksi        ENUM('create', 'update', 'delete', 'restore') NOT NULL,
    entitas     ENUM('pekerjaan', 'pemanfaat', 'foto') NOT NULL,
    entitas_id  BIGINT UNSIGNED NOT NULL,
    project_id  BIGINT UNSIGNED NULL,
    keterangan  VARCHAR(500) NOT NULL,
    INDEX idx_log_waktu (waktu),
    INDEX idx_log_project (project_id),
    CONSTRAINT fk_log_user FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE SET NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;
