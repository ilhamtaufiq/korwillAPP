//! Penyimpanan isian Form Laporan Mingguan yang tidak berasal dari Lapming:
//! tanggal tahapan, target per lokasi, partisipan masyarakat, dan permasalahan.
//!
//! Kontrak (dipakai oleh API, impor, dan generator):
//! - Lokasi dikenali dengan `kunci_lokasi(kab, kec, desa, tahap, nama)`. Nama hanya dipakai untuk LPK.
//! - Tahapan: kunci nomor tahap ("1".."27" untuk Sanimas, "1".."24" untuk LPK), nilai "YYYY-MM-DD".
//! - Target: kunci huruf kolom template ("G", "H", ...), nilai teks apa adanya (angka tetap ditulis sebagai teks).
//! - Partisipan dan permasalahan: satu baris = peta kolom huruf -> nilai teks.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::lapming::fmt::norm_nama;

/// (kolom template, label) untuk tahapan Sanimas, urut dari kolom I.
pub const TAHAP_SAN: [(&str, &str); 27] = [
    ("I", "Sosialisasi Awal Tingkat Provinsi"),
    ("J", "Koordinasi Awal dan Pengumpulan Data Sekunder"),
    ("K", "Sosialiasasi Awal Desa"),
    ("L", "Kampanye PHBS I"),
    ("M", "Identifikasi Calon Penerima Manfaat"),
    ("N", "Pembentukan Kelembagaan KMP"),
    ("O", "SK KMP oleh KPA"),
    ("P", "Penetapan Penerima Pemanfaat"),
    ("Q", "Penyusunan Rencana Kerja Masyarakat (RKM)"),
    ("R", "Pembukaan Rekening"),
    ("S", "SK Penetapan Penerima Pemanfaat"),
    ("T", "Verikasi RKM"),
    ("U", "Finalisasi RKM"),
    ("V", "Perjanjian Kerja Sama"),
    ("W", "Pengadaan Barjas"),
    ("X", "Penyaluran T1"),
    ("Y", "Peningkatan Kapasitas KMP & Pelatihan Tenaga Kerja"),
    ("Z", "Pelaksanaan Konstruksi"),
    ("AA", "Kampanye PHBS II"),
    ("AB", "Commisioning Test I"),
    ("AC", "Penyaluran T2"),
    ("AD", "Penyusunan & Sosialisasi SOP Operasional dan Pemeliharaan"),
    ("AE", "Kampanye PHBS III"),
    ("AF", "Fisik 100%"),
    ("AG", "Commisioning Test II"),
    ("AH", "Penutupan Rekening"),
    ("AI", "Serah Terima"),
];

/// (kolom template, label) untuk tahapan LPK, urut dari kolom I.
pub const TAHAP_LPK: [(&str, &str); 28] = [
    ("I", "Sosialisasi Awal Tingkat Provinsi"),
    ("J", "Koordinasi Awal dan Pengumpulan Data Sekunder"),
    ("K", "Sosialiasasi Awal LPK / Lembaga"),
    ("L", "Kampanye PHBS I"),
    ("M", "Penyesuaian Desain LPK Kontraktual"),
    ("N", "Pemilihan Penyedia Jasa Konstruksi Kontraktual"),
    ("O", "Pemilihan Konsultan Supervisi Kontraktual"),
    ("P", "Pembentukan Kelembagaan KMP"),
    ("Q", "SK POKMAS oleh KPA"),
    ("R", "Penyusunan Rencana Kerja Masyarakat (RKM) Swakelola"),
    ("S", "Pembukaan Rekening Swakelola"),
    ("T", "SK Penetapan Penerima Pemanfaat"),
    ("U", "Verikasi RKM Swakelola"),
    ("V", "Finalisasi RKM Swakelola"),
    ("W", "Perjanjian Kerja Sama"),
    ("X", "Pengadaan Barjas"),
    ("Y", "Penyaluran T1 / Uang Muka"),
    ("Z", "Peningkatan Kapasitas KMP & Pelatihan Tenaga Kerja Swakelola"),
    ("AA", "Pelaksanaan Konstruksi"),
    ("AB", "Pembentukan KMP Pengelolaan LPK Kontraktual"),
    ("AC", "Penyaluran T2 / Pelunasan"),
    ("AD", "Commisioning Test"),
    ("AE", "Penyusunan & Sosialisasi SOP Operasional dan Pemeliharaan"),
    ("AF", "Kampanye PHBS II"),
    ("AG", "Pengukuhan KMP Kontraktual"),
    ("AH", "Fisik 100%"),
    ("AI", "Penutupan Rekening Swakelola"),
    ("AJ", "Serah Terima"),
];

/// (kolom template, label) untuk target Sanimas.
pub const TARGET_SAN: [(&str, &str); 32] = [
    ("G", "Longlist (KK)"),
    ("H", "Rencana Output (KK)"),
    ("I", "Rencana Output (Jiwa)"),
    ("J", "Rencana Unit Terbangun (Unit)"),
    ("K", "Rencana Jumlah Bilik (Unit)"),
    ("L", "Rencana Harga Per Bilik (x 1000)"),
    ("M", "Rencana Jumlah Greasetrap (Unit)"),
    ("N", "Rencana Harga Per Greasetrap (x 1000)"),
    ("O", "Rencana Jumlah Resapan (Unit)"),
    ("P", "Rencana Harga Per Resapan (x 1000)"),
    ("Q", "Rencana Jumlah TS Individu (Unit)"),
    ("R", "Rencana Jumlah TS Komunal (Unit)"),
    ("S", "Rencana Harga Per TS Individu (x 1000)"),
    ("T", "Rencana Harga Per TS Komunal (x 1000)"),
    ("U", "Rencana Jumlah IPALD/TS Konvensional (Unit)"),
    ("V", "Rencana Jumlah IPALD/TS Pabrikasi (Unit)"),
    ("W", "Lokasi Daratan/ Kepulauan"),
    ("X", "Daerah Normal (Unit)"),
    ("Y", "Daerah Rawa (Unit)"),
    ("Z", "Daerah Gambut (Unit)"),
    ("AA", "Daerah Perairan (Unit)"),
    ("AB", "Jenis Konstruksi dan Pondasi Pondasi Menerus (Batu belah) (Unit)"),
    ("AC", "Jenis Konstruksi dan Pondasi Pondasi Setempat (plat beton) (Unit)"),
    ("AD", "Jenis Konstruksi dan Pondasi Struktur & Pondasi kayu (Unit)"),
    ("AE", "Pemanfaat DTSEN Desil 1-4 (KK)"),
    ("AF", "Pemanfaat DTSEN Desil 5 (KK)"),
    ("AG", "Pemanfaat Perempuan (jiwa)"),
    ("AH", "Pemanfaat Bumil (jiwa)"),
    ("AI", "Pemanfaat Baduta (jiwa)"),
    ("AJ", "Pemanfaat Stunting (jiwa)"),
    ("AK", "Pemanfaat Disabilitas (jiwa)"),
    ("AL", "Pemanfaat Lansia (jiwa)"),
];

/// (kolom template, label) untuk target LPK.
pub const TARGET_LPK: [(&str, &str); 13] = [
    ("H", "Rencana Output (Jiwa)"),
    ("I", "Rencana Jumlah Bilik (Unit MCK)"),
    ("J", "Rencana Harga Per Bilik (x 1000)"),
    ("K", "Rencana Jumlah IPALD Konvensional / Pabrikasi"),
    ("L", "Rencana IPALD (Jml Unit)"),
    ("M", "Rencana Harga Per IPAL (x 1000)"),
    ("N", "Kesesuaian Dg Prototipe (Ya / Tidak)"),
    ("O", "Islam/ Kristen/ Katolik/ Budha/ Hindu/ Konghuchu"),
    ("P", "Desain Laki-Laki/ Perempuan"),
    ("Q", "Prototype MCK (Mukim /Non Mukim/ Disabilitas)"),
    ("R", "Yayasan/ Non Yayasan"),
    ("S", "Lokasi Daratan/ Kepulauan"),
    ("T", "Normal/ Daerah Rawa/ Gambut/ Perairan"),
];

/// (kolom template, label) untuk partisipan masyarakat.
pub const PARTISIPAN_KOLOM: [(&str, &str); 36] = [
    ("A", "KEGIATAN"),
    ("B", "PROVINSI"),
    ("C", "KABUPATEN"),
    ("D", "KECAMATAN"),
    ("E", "KELURAHAN/ DESA"),
    ("F", "NAMA LPK"),
    ("G", "JENIS KONTRAK"),
    ("H", "STATUS"),
    ("I", "PIC Korwil"),
    ("J", "Laki-Laki"),
    ("K", "Perempuan"),
    ("L", "Jumlah Peserta OPD terkait Kesehatan"),
    ("M", "Jumlah Peserta OPD terkait Persampahan"),
    ("N", "Jumlah Peserta OPD terkait Air Limbah"),
    ("O", "Jumlah Peserta Kemenag"),
    ("P", "Laki-Laki"),
    ("Q", "Perempuan"),
    ("R", "Warga MBR"),
    ("S", "Laki-Laki"),
    ("T", "Perempuan"),
    ("U", "Warga MBR"),
    ("V", "Laki-Laki"),
    ("W", "Perempuan"),
    ("X", "Warga MBR"),
    ("Y", "Jumlah Anggota Laki-Laki"),
    ("Z", "Jumlah Anggota Perempuan"),
    ("AA", "Jumlah Anggota MBR"),
    ("AB", "Laki-Laki"),
    ("AC", "Perempuan"),
    ("AD", "Warga MBR"),
    ("AE", "Laki-Laki"),
    ("AF", "Perempuan"),
    ("AG", "Warga MBR"),
    ("AH", "Laki-Laki"),
    ("AI", "Perempuan"),
    ("AJ", "Warga MBR"),
];

/// (kolom template, label) untuk permasalahan.
pub const PERMASALAHAN_KOLOM: [(&str, &str); 12] = [
    ("B", "Kabupaten/Kota"),
    ("C", "Kecamatan"),
    ("D", "Desa/Kelurahan"),
    ("E", "Nama KMP"),
    ("F", "Kegiatan"),
    ("G", "Permasalahan dan Penyebab"),
    ("H", "Tanggal Kejadian Diketahui"),
    ("I", "Rencana Tindak Lanjut"),
    ("J", "Tindak Lanjut"),
    ("K", "Tanggal Pelaksanaan Tindaklanjut"),
    ("L", "Target Penyelesaian (Minggu ke…Bulan….)"),
    ("M", "Status Permasalahan pada periode pelaporan"),
];

/// Isian satu lokasi (satu baris Tahapan/Target).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LokasiData {
    /// Nomor tahap (sebagai teks) -> tanggal "YYYY-MM-DD".
    #[serde(default)]
    pub tahapan: BTreeMap<String, String>,
    /// Huruf kolom template -> nilai teks.
    #[serde(default)]
    pub target: BTreeMap<String, String>,
}

/// Data form per korwil, dikelompokkan siap pakai.
#[derive(Clone, Debug, Default)]
pub struct FormData {
    /// (jenis "san"/"lpk", kunci lokasi) -> isian.
    pub lokasi: HashMap<(String, String), LokasiData>,
    pub partisipan: Vec<BTreeMap<String, String>>,
    pub permasalahan: Vec<BTreeMap<String, String>>,
}

/// Kunci lokasi yang stabil: huruf kecil, tanpa spasi berlebih. Nama Pokmas/LPK ikut hanya untuk LPK.
pub fn kunci_lokasi(kab: &str, kec: &str, desa: &str, tahap: &str, nama: &str) -> String {
    [kab, kec, desa, tahap, nama].iter().map(|s| norm_nama(s)).collect::<Vec<_>>().join("|")
}

/// Membaca semua isian form untuk satu korwil.
pub async fn muat(db: &MySqlPool, korwil: &str) -> Result<FormData, AppError> {
    let mut out = FormData::default();
    let lokasi: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT CAST(jenis AS CHAR), lokasi, tahapan, target FROM form_lokasi_data WHERE korwil = ?",
    )
    .bind(korwil)
    .fetch_all(db)
    .await?;
    for (jenis, kunci, tahapan, target) in lokasi {
        let data = LokasiData {
            tahapan: serde_json::from_str(&tahapan).unwrap_or_default(),
            target: serde_json::from_str(&target).unwrap_or_default(),
        };
        out.lokasi.insert((jenis, kunci), data);
    }
    let baris: Vec<(String, u32, String)> = sqlx::query_as(
        "SELECT CAST(bagian AS CHAR), urut, data FROM form_baris WHERE korwil = ? ORDER BY bagian, urut",
    )
    .bind(korwil)
    .fetch_all(db)
    .await?;
    for (bagian, _urut, data) in baris {
        let peta: BTreeMap<String, String> = serde_json::from_str(&data).unwrap_or_default();
        match bagian.as_str() {
            "partisipan" => out.partisipan.push(peta),
            "permasalahan" => out.permasalahan.push(peta),
            _ => {}
        }
    }
    Ok(out)
}

/// Menyimpan isian satu lokasi (menimpa yang lama).
pub async fn simpan_lokasi(db: &MySqlPool, korwil: &str, jenis: &str, kunci: &str, data: &LokasiData) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO form_lokasi_data (korwil, jenis, lokasi, tahapan, target) VALUES (?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE tahapan = VALUES(tahapan), target = VALUES(target)",
    )
    .bind(korwil)
    .bind(jenis)
    .bind(kunci)
    .bind(serde_json::to_string(&data.tahapan).map_err(|e| AppError::BadRequest(e.to_string()))?)
    .bind(serde_json::to_string(&data.target).map_err(|e| AppError::BadRequest(e.to_string()))?)
    .execute(db)
    .await?;
    Ok(())
}

/// Mengganti seluruh daftar partisipan atau permasalahan satu korwil.
pub async fn ganti_baris(db: &MySqlPool, korwil: &str, bagian: &str, daftar: &[BTreeMap<String, String>]) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM form_baris WHERE korwil = ? AND bagian = ?")
        .bind(korwil)
        .bind(bagian)
        .execute(&mut *tx)
        .await?;
    for (i, peta) in daftar.iter().enumerate() {
        sqlx::query("INSERT INTO form_baris (korwil, bagian, urut, data) VALUES (?, ?, ?, ?)")
            .bind(korwil)
            .bind(bagian)
            .bind((i + 1) as u32)
            .bind(serde_json::to_string(peta).map_err(|e| AppError::BadRequest(e.to_string()))?)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kunci_lokasi_tidak_peka_huruf_dan_spasi() {
        assert_eq!(
            kunci_lokasi("Bogor", "Ciomas", "Pagelaran", "Tahap 1", ""),
            kunci_lokasi(" bogor ", "CIOMAS", "pagelaran ", "tahap 1", "")
        );
    }

    #[test]
    fn jumlah_kolom_sesuai_template() {
        assert_eq!(TAHAP_SAN.len(), 27);
        assert_eq!(TAHAP_LPK.len(), 28);
    }
}
