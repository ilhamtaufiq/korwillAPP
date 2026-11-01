-- Dua fasilitator masyarakat per pekerjaan: FM Teknik (kolom tfl_nama) dan FM Pemberdayaan (kolom ini).
-- Dari rekap Lapming, kolom "NAMA TFL / PENDAMPING LAPANGAN" dipecah menjadi TEKNIK dan PEMBERDAYAAN.
ALTER TABLE projects
    ADD COLUMN fm_pemberdayaan_nama VARCHAR(255) NULL AFTER tfl_nama;
