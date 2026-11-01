-- Penanda wajib ganti kata sandi. Akun yang dibuat dari impor xlsx memakai password = username,
-- jadi harus diganti setelah login pertama.
ALTER TABLE users
    ADD COLUMN must_change_password TINYINT(1) NOT NULL DEFAULT 0;
