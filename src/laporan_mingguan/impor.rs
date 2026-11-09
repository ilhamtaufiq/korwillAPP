//! Membaca form mingguan lama (.xlsx yang sudah diisi) menjadi isian aplikasi.
//! STUB: diisi oleh pekerjaan impor.

use std::collections::BTreeMap;

use super::store::LokasiData;

/// Hasil baca form lama: isian lokasi per (jenis, kunci), partisipan, dan permasalahan.
#[derive(Debug, Default)]
pub struct ImporForm {
    pub lokasi: Vec<(String, String, LokasiData)>,
    pub partisipan: Vec<BTreeMap<String, String>>,
    pub permasalahan: Vec<BTreeMap<String, String>>,
}

pub fn baca(_bytes: &[u8]) -> Result<ImporForm, String> {
    Err("Impor belum tersedia".to_string())
}
