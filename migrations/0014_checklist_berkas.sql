-- Checklist berkas per pokmas (Lembar Kendali POKMAS SPALD-S). Info pokmas dan centang disimpan sebagai JSON.
CREATE TABLE checklist_pokmas (
    korwil     VARCHAR(255) NOT NULL,
    kunci      VARCHAR(512) NOT NULL,
    info       LONGTEXT NOT NULL,
    centang    LONGTEXT NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (korwil, kunci)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;
