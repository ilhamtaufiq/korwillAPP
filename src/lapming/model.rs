//! Model data Lapming: satu baris Pokmas per sheet minggu, dan ringkasan hasil hitung.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Kumulatif HOK, upah, material, BOP dari M1 sampai minggu terpilih.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cum {
    pub q: f64,
    pub r: f64,
    pub s: f64,
    pub u: f64,
    pub w: f64,
    pub aa: f64,
    pub z: f64,
}

/// Satu baris Pokmas di satu sheet minggu. Nama field mengikuti huruf kolom di skrip Python.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Row {
    pub lpk: bool,
    pub sheet: String,
    pub row: u32,
    pub no: String,
    pub nama: String,
    pub label: String,
    pub desa: String,
    pub tahap: Option<String>,
    pub kab: Option<String>,
    pub kec: Option<String>,
    pub korfas: String,
    pub pagu: f64,
    pub mulai: Option<NaiveDate>,
    pub selesai: Option<NaiveDate>,
    pub p: i64,
    pub q: i64,
    pub r: i64,
    pub s: f64,
    pub u: f64,
    pub w: f64,
    pub y: f64,
    pub z: f64,
    pub ab: f64,
    pub ac: f64,
    pub ae: f64,
    pub af: f64,
    pub aa: f64,
    pub ad: f64,
    pub ag: f64,
    /// Target kurva S; kosong tetap kosong.
    pub ah: Option<f64>,
    /// Data wajib yang kosong (nama kolom), dan data lain yang kosong.
    pub wajib: Vec<String>,
    pub info: Vec<String>,
    #[serde(default)]
    pub b: Cum,
    /// Jumlah RPDB per tahap (None bila belum ditarik) dan totalnya.
    #[serde(default)]
    pub rpdb_t: Vec<Option<i64>>,
    #[serde(default)]
    pub rpdb: Option<i64>,
}

impl Row {
    /// Deviasi fisik terhadap target; hanya ada bila target terisi.
    pub fn ai(&self) -> Option<f64> {
        self.ah.map(|t| self.ag - t)
    }

    /// Baris sudah diisi untuk minggu ini (ada pekerja, dana minggu ini, atau tanggal pelaksanaan).
    pub fn terisi(&self) -> bool {
        self.p > 0 || self.z > 0.0 || self.mulai.is_some()
    }

    pub fn jenis(&self) -> &'static str {
        if self.lpk {
            "LPK"
        } else {
            "Sanimas"
        }
    }
}

/// Satu workbook sumber (Sanimas atau LPK): semua sheet minggu, semua korfas dan tahap.
#[derive(Clone, Debug, Default)]
pub struct Book {
    /// Nama sheet M<n> yang ada di file (juga yang gagal dibaca).
    pub names: Vec<String>,
    /// Nomor minggu -> baris. Sheet yang header-nya tidak lengkap tidak ada di sini.
    pub sheets: BTreeMap<u32, Vec<Row>>,
}

/// Dua sumber data. Salah satunya boleh kosong.
#[derive(Clone, Debug, Default)]
pub struct Dataset {
    pub san: Option<Book>,
    pub lpk: Option<Book>,
}

impl Dataset {
    /// (apakah LPK, workbook) untuk setiap sumber yang ada, Sanimas lebih dulu.
    pub fn books(&self) -> impl Iterator<Item = (bool, &Book)> {
        self.san
            .iter()
            .map(|b| (false, b))
            .chain(self.lpk.iter().map(|b| (true, b)))
    }
}

/// Satu temuan: (indeks baris, kategori, keterangan).
pub type Temuan = (usize, String, String);

/// Ringkasan hasil hitung untuk satu minggu (S di skrip Python). Daftar menyimpan indeks ke `rows`.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub san: Vec<usize>,
    pub lpk: Vec<usize>,
    pub n: usize,
    pub pagu_s: f64,
    pub pagu_l: f64,
    pub pagu: f64,
    pub real_s: f64,
    pub real_l: f64,
    pub real: f64,
    pub real_mgg: f64,
    pub pekerja: i64,
    pub hok: i64,
    pub fis: f64,
    pub fis_s: f64,
    pub fis_l: f64,
    pub naik: f64,
    pub n_target: usize,
    pub tgt: Option<f64>,
    pub fis_cmp: Option<f64>,
    pub dev: Vec<usize>,
    pub per: String,
    pub keu: f64,
    pub keu_s: f64,
    pub keu_l: f64,
    pub neg: Vec<usize>,
    pub worst: Option<usize>,
    pub nol: Vec<usize>,
    pub min_p: usize,
    pub keu_dulu: usize,
    /// Baris dengan isian kosong: (indeks, wajib, info). Data wajib kosong lebih dulu.
    pub catatan: Vec<(usize, Vec<String>, Vec<String>)>,
    /// Bagian catatan yang data wajibnya kosong.
    pub kurang: Vec<(usize, Vec<String>, Vec<String>)>,
    pub rpdb: Option<i64>,
    pub masalah: Vec<Temuan>,
    pub perhatian: Vec<Temuan>,
}

/// Nilai yang dilacak untuk log perubahan.
pub const TRACK: [&str; 16] = [
    "mulai", "selesai", "P", "Q", "R", "S", "U", "W", "Z", "AA", "AC", "AD", "AF", "AG", "AH", "AI",
];

pub fn field_nama(f: &str) -> &'static str {
    match f {
        "mulai" => "Mulai",
        "selesai" => "Selesai",
        "P" => "Pekerja",
        "Q" => "Hari kerja",
        "R" => "HOK",
        "S" => "Upah",
        "U" => "Material",
        "W" => "BOP",
        "Z" => "Dana mgg ini",
        "AA" => "Dana kum.",
        "AC" => "Keu. mgg ini",
        "AD" => "Keu. kum.",
        "AF" => "Fisik mgg ini",
        "AG" => "Fisik kum.",
        "AH" => "Target",
        "AI" => "Deviasi",
        _ => "",
    }
}
