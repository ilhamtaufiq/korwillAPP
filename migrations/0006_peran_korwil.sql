-- Peran korwil (KORFAS) ditambahkan. Akun korwil dihubungkan ke pekerjaan lewat nama seperti TFL.
ALTER TABLE users
    MODIFY COLUMN role ENUM('admin', 'tfl', 'korwil') NOT NULL;
