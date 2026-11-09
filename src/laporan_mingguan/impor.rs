//! Membaca form mingguan lama (.xlsx yang sudah diisi) menjadi isian aplikasi.
//!
//! Tata letak mengikuti `templates/laporan_mingguan.xlsx`:
//! - Tahapan Sanimas: data mulai baris 12, C..F = kab, kec, desa, tahap; tanggal tahap di I..AI.
//! - Tahapan LPK: data mulai baris 11, C..G = kab, kec, desa, nama LPK, tahap; tanggal tahap di I..AJ.
//! - Target Sanimas: baris 10+i berpadanan dengan Tahapan Sanimas baris 12+i; nilai di G..AL.
//! - Target LPK: baris 10+i berpadanan dengan Tahapan LPK baris 11+i; nilai di H..T.
//! - Partisipan Masyarakat: data mulai baris 9, kolom A..AJ.
//! - Permasalahan: data mulai baris 10, kolom B..M (A = nomor, diabaikan).
//!
//! Sel rumus dibaca dari nilai hasil hitung yang tersimpan di file (cache), kecuali di sheet Target:
//! di sana hanya sel konstanta yang diambil.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use calamine::{open_workbook_from_rs, Data, Range, Reader, Xlsx};
use chrono::{Duration, NaiveDate};
use regex::Regex;

use super::store::{self, LokasiData};

/// Hasil baca form lama: isian lokasi per (jenis, kunci), partisipan, dan permasalahan.
#[derive(Debug, Default)]
pub struct ImporForm {
    pub lokasi: Vec<(String, String, LokasiData)>,
    pub partisipan: Vec<BTreeMap<String, String>>,
    pub permasalahan: Vec<BTreeMap<String, String>>,
}

const SHEET_TAHAP_SAN: &str = "Tahapan Sanimas";
const SHEET_TAHAP_LPK: &str = "Tahapan LPK";
const SHEET_TARGET_SAN: &str = "Target Sanimas";
const SHEET_TARGET_LPK: &str = "Target LPK";
const SHEET_PARTISIPAN: &str = "Partisipan Masyarakat";
const SHEET_PERMASALAHAN: &str = "Permasalahan";

/// Kapasitas baris lokasi di template (sama dengan generator).
const BARIS_SAN: u32 = 50;
const BARIS_LPK: u32 = 45;

/// Baris pertama data (nomor baris Excel, mulai 1).
const AWAL_TAHAP_SAN: u32 = 12;
const AWAL_TAHAP_LPK: u32 = 11;
const AWAL_TARGET: u32 = 10;
const AWAL_PARTISIPAN: u32 = 9;
const AWAL_PERMASALAHAN: u32 = 10;

/// Satu sheet: nilai (termasuk cache rumus) dan teks rumusnya.
struct Lembar {
    nilai: Range<Data>,
    rumus: Range<String>,
}

impl Lembar {
    /// Sel berdasarkan huruf kolom dan nomor baris Excel (mulai 1).
    fn sel(&self, kolom: &str, baris: u32) -> &Data {
        self.nilai.get_value((baris - 1, indeks_kolom(kolom))).unwrap_or(&Data::Empty)
    }

    fn ada_rumus(&self, kolom: &str, baris: u32) -> bool {
        self.rumus
            .get_value((baris - 1, indeks_kolom(kolom)))
            .is_some_and(|f| !f.trim().is_empty())
    }

    fn teks(&self, kolom: &str, baris: u32) -> String {
        teks_sel(self.sel(kolom, baris))
    }

    /// Nomor baris Excel terakhir yang memuat nilai.
    fn baris_akhir(&self) -> u32 {
        self.nilai.end().map(|(r, _)| r + 1).unwrap_or(0)
    }

    /// Baris penutup tabel ("Jumlah", "Keterangan :").
    fn baris_penutup(&self, baris: u32) -> bool {
        ["A", "C"].iter().any(|k| {
            let t = self.teks(k, baris).to_lowercase();
            t.starts_with("jumlah") || t.starts_with("keterangan")
        })
    }
}

/// "A" -> 0, "Z" -> 25, "AA" -> 26.
fn indeks_kolom(kolom: &str) -> u32 {
    kolom.bytes().fold(0u32, |n, b| n * 26 + (b - b'A' + 1) as u32) - 1
}

fn angka_teks(f: f64) -> String {
    if f.is_finite() && f == f.trunc() && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// Nilai sel sebagai teks, dipangkas. Tanggal menjadi "YYYY-MM-DD"; error menjadi kosong.
fn teks_sel(d: &Data) -> String {
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => angka_teks(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(dt) => match serial_ke_tanggal(dt.as_f64()) {
            Some(t) => t.format("%Y-%m-%d").to_string(),
            None => angka_teks(dt.as_f64()),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.trim().to_string(),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

fn serial_ke_tanggal(serial: f64) -> Option<NaiveDate> {
    if !serial.is_finite() || serial < 1.0 {
        return None;
    }
    NaiveDate::from_ymd_opt(1899, 12, 30)?.checked_add_signed(Duration::days(serial.floor() as i64))
}

fn bulan_angka(nama: &str) -> Option<u32> {
    Some(match nama {
        "januari" | "jan" | "january" => 1,
        "februari" | "feb" | "pebruari" | "february" => 2,
        "maret" | "mar" | "march" => 3,
        "april" | "apr" => 4,
        "mei" | "may" => 5,
        "juni" | "jun" | "june" => 6,
        "juli" | "jul" | "july" => 7,
        "agustus" | "agu" | "agt" | "agus" | "aug" | "august" => 8,
        "september" | "sep" | "sept" => 9,
        "oktober" | "okt" | "oct" | "october" => 10,
        "november" | "nov" | "nop" | "nopember" => 11,
        "desember" | "des" | "dec" | "december" => 12,
        _ => return None,
    })
}

/// Tanggal dari teks: "21 Agustus 2026", "21-Agu-2026", "4/18/2025" (bulan/hari/tahun;
/// ditukar bila angka pertama > 12), atau "2026-08-21".
fn tanggal_teks(s: &str) -> Option<NaiveDate> {
    let s = s.trim().to_lowercase();
    let re_iso = Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2})").unwrap();
    if let Some(m) = re_iso.captures(&s) {
        return NaiveDate::from_ymd_opt(m[1].parse().ok()?, m[2].parse().ok()?, m[3].parse().ok()?);
    }
    let re_num = Regex::new(r"^(\d{1,2})[/.-](\d{1,2})[/.-](\d{2,4})$").unwrap();
    if let Some(m) = re_num.captures(&s) {
        let a: u32 = m[1].parse().ok()?;
        let b: u32 = m[2].parse().ok()?;
        let mut y: i32 = m[3].parse().ok()?;
        if y < 100 {
            y += 2000;
        }
        let (bulan, hari) = if a > 12 { (b, a) } else { (a, b) };
        return NaiveDate::from_ymd_opt(y, bulan, hari);
    }
    let re_nama = Regex::new(r"^(\d{1,2})[\s.\-/]*([a-z]+)\.?[\s.\-/,]*(\d{4})$").unwrap();
    if let Some(m) = re_nama.captures(&s) {
        return NaiveDate::from_ymd_opt(m[3].parse().ok()?, bulan_angka(&m[2])?, m[1].parse().ok()?);
    }
    None
}

/// Sel tanggal tahapan menjadi "YYYY-MM-DD"; sel kosong atau tidak dikenali menjadi None.
fn tanggal_sel(d: &Data) -> Option<String> {
    let t = match d {
        Data::DateTime(dt) => serial_ke_tanggal(dt.as_f64()),
        // Tanggal yang tersimpan sebagai angka biasa (sel tanpa format tanggal).
        Data::Float(f) if (1000.0..200_000.0).contains(f) => serial_ke_tanggal(*f),
        Data::Int(i) if (1000..200_000).contains(i) => serial_ke_tanggal(*i as f64),
        Data::DateTimeIso(s) | Data::String(s) => tanggal_teks(s),
        _ => None,
    }?;
    Some(t.format("%Y-%m-%d").to_string())
}

fn buka_lembar(wb: &mut Xlsx<Cursor<&[u8]>>, nama: &str) -> Result<Lembar, String> {
    let sebenarnya = wb
        .sheet_names()
        .into_iter()
        .find(|s| s.trim().eq_ignore_ascii_case(nama))
        .ok_or_else(|| format!("Sheet '{nama}' tidak ditemukan. Pastikan file adalah Form Laporan Mingguan."))?;
    let nilai = wb
        .worksheet_range(&sebenarnya)
        .map_err(|e| format!("Sheet '{nama}' tidak dapat dibaca: {e}"))?;
    // Rumus hanya dipakai untuk mengenali sel rumus; kalau gagal dibaca, anggap tidak ada rumus.
    let rumus = wb.worksheet_formula(&sebenarnya).unwrap_or_default();
    Ok(Lembar { nilai, rumus })
}

/// Isian lokasi yang sudah terkumpul, urut sesuai kemunculan; lokasi kembar digabung.
#[derive(Default)]
struct Kumpulan {
    daftar: Vec<(String, String, LokasiData)>,
    posisi: HashMap<(String, String), usize>,
}

impl Kumpulan {
    fn tambah(&mut self, jenis: &str, kunci: String, data: LokasiData) {
        if data.tahapan.is_empty() && data.target.is_empty() {
            return;
        }
        match self.posisi.get(&(jenis.to_string(), kunci.clone())) {
            Some(&i) => {
                let lama = &mut self.daftar[i].2;
                lama.tahapan.extend(data.tahapan);
                lama.target.extend(data.target);
            }
            None => {
                self.posisi.insert((jenis.to_string(), kunci.clone()), self.daftar.len());
                self.daftar.push((jenis.to_string(), kunci, data));
            }
        }
    }
}

/// Nilai target satu baris: hanya sel konstanta, tanpa "-" dan kosong.
fn target_baris(lembar: &Lembar, baris: u32, kolom: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, _) in kolom {
        if lembar.ada_rumus(k, baris) {
            continue;
        }
        let v = lembar.teks(k, baris);
        if v.is_empty() || v == "-" {
            continue;
        }
        out.insert(k.to_string(), v);
    }
    out
}

fn tahapan_baris(lembar: &Lembar, baris: u32, kolom: &[(&str, &str)]) -> BTreeMap<String, String> {
    kolom
        .iter()
        .enumerate()
        .filter_map(|(i, (k, _))| tanggal_sel(lembar.sel(k, baris)).map(|t| ((i + 1).to_string(), t)))
        .collect()
}

/// Peta kolom -> teks untuk satu baris tabel; sel kosong dilewati.
fn baris_tabel(lembar: &Lembar, baris: u32, kolom: &[(&str, &str)]) -> BTreeMap<String, String> {
    kolom
        .iter()
        .filter_map(|(k, _)| {
            let v = lembar.teks(k, baris);
            (!v.is_empty()).then(|| (k.to_string(), v))
        })
        .collect()
}

pub fn baca(bytes: &[u8]) -> Result<ImporForm, String> {
    let mut wb: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes)).map_err(|_| {
        "File tidak dapat dibaca sebagai Excel (.xlsx). Pastikan file adalah Form Laporan Mingguan.".to_string()
    })?;
    let tahap_san = buka_lembar(&mut wb, SHEET_TAHAP_SAN)?;
    let target_san = buka_lembar(&mut wb, SHEET_TARGET_SAN)?;
    let tahap_lpk = buka_lembar(&mut wb, SHEET_TAHAP_LPK)?;
    let target_lpk = buka_lembar(&mut wb, SHEET_TARGET_LPK)?;
    let partisipan = buka_lembar(&mut wb, SHEET_PARTISIPAN)?;
    let permasalahan = buka_lembar(&mut wb, SHEET_PERMASALAHAN)?;

    let mut lokasi = Kumpulan::default();

    // Sanimas.
    for i in 0..BARIS_SAN {
        let t = AWAL_TAHAP_SAN + i;
        if tahap_san.baris_penutup(t) {
            break;
        }
        let (kab, kec, desa, tahap) =
            (tahap_san.teks("C", t), tahap_san.teks("D", t), tahap_san.teks("E", t), tahap_san.teks("F", t));
        if kab.is_empty() && kec.is_empty() && desa.is_empty() {
            continue;
        }
        let g = AWAL_TARGET + i;
        let data = LokasiData {
            tahapan: tahapan_baris(&tahap_san, t, &store::TAHAP_SAN),
            target: if target_san.baris_penutup(g) {
                BTreeMap::new()
            } else {
                target_baris(&target_san, g, &store::TARGET_SAN)
            },
        };
        lokasi.tambah("san", store::kunci_lokasi(&kab, &kec, &desa, &tahap, ""), data);
    }

    // LPK.
    for i in 0..BARIS_LPK {
        let t = AWAL_TAHAP_LPK + i;
        if tahap_lpk.baris_penutup(t) {
            break;
        }
        let (kab, kec, desa, nama, tahap) = (
            tahap_lpk.teks("C", t),
            tahap_lpk.teks("D", t),
            tahap_lpk.teks("E", t),
            tahap_lpk.teks("F", t),
            tahap_lpk.teks("G", t),
        );
        if kab.is_empty() && kec.is_empty() && desa.is_empty() && nama.is_empty() {
            continue;
        }
        let g = AWAL_TARGET + i;
        let data = LokasiData {
            tahapan: tahapan_baris(&tahap_lpk, t, &store::TAHAP_LPK),
            target: if target_lpk.baris_penutup(g) {
                BTreeMap::new()
            } else {
                target_baris(&target_lpk, g, &store::TARGET_LPK)
            },
        };
        lokasi.tambah("lpk", store::kunci_lokasi(&kab, &kec, &desa, &tahap, &nama), data);
    }

    // Partisipan: baris tanpa lokasi (C..F) dianggap kosong, termasuk baris yang hanya berisi hasil rumus
    // seperti provinsi.
    let mut daftar_partisipan = Vec::new();
    for b in AWAL_PARTISIPAN..=partisipan.baris_akhir() {
        if partisipan.baris_penutup(b) {
            break;
        }
        let peta = baris_tabel(&partisipan, b, &store::PARTISIPAN_KOLOM);
        if ["C", "D", "E", "F"].iter().any(|k| peta.contains_key(*k)) {
            daftar_partisipan.push(peta);
        }
    }

    // Permasalahan: baris tanpa lokasi, nama KMP, kegiatan, maupun uraian masalah (B..G) dilewati,
    // misalnya sisa contoh pengisian yang hanya berisi tanggal dan status.
    let mut daftar_permasalahan = Vec::new();
    for b in AWAL_PERMASALAHAN..=permasalahan.baris_akhir() {
        let peta = baris_tabel(&permasalahan, b, &store::PERMASALAHAN_KOLOM);
        if ["B", "C", "D", "E", "F", "G"].iter().any(|k| peta.contains_key(*k)) {
            daftar_permasalahan.push(peta);
        }
    }

    Ok(ImporForm { lokasi: lokasi.daftar, partisipan: daftar_partisipan, permasalahan: daftar_permasalahan })
}

#[cfg(test)]
mod tests {
    use super::super::isi::{isi_template, Isian, Nilai};
    use super::*;

    fn teks(sheet: &str, sel: &str, v: &str) -> Isian {
        (sheet.to_string(), sel.to_string(), Nilai::Teks(v.to_string()))
    }

    fn angka(sheet: &str, sel: &str, v: f64) -> Isian {
        (sheet.to_string(), sel.to_string(), Nilai::Angka(v))
    }

    fn cari<'a>(hasil: &'a ImporForm, jenis: &str, kunci: &str) -> &'a LokasiData {
        &hasil
            .lokasi
            .iter()
            .find(|(j, k, _)| j == jenis && k == kunci)
            .unwrap_or_else(|| panic!("lokasi {jenis} {kunci} tidak ada di {:?}", hasil.lokasi))
            .2
    }

    #[test]
    fn tanggal_teks_dikenali() {
        let t = |s: &str| tanggal_teks(s).map(|d| d.format("%Y-%m-%d").to_string());
        assert_eq!(t("21 Agustus 2026").as_deref(), Some("2026-08-21"));
        assert_eq!(t(" 3 Nopember 2026 ").as_deref(), Some("2026-11-03"));
        assert_eq!(t("21-Agu-2026").as_deref(), Some("2026-08-21"));
        assert_eq!(t("4/18/2025").as_deref(), Some("2025-04-18"));
        assert_eq!(t("18/4/2025").as_deref(), Some("2025-04-18"));
        assert_eq!(t("2026-08-21").as_deref(), Some("2026-08-21"));
        assert_eq!(t("tgl-bln-th"), None);
        assert_eq!(t("31 Februari 2026"), None);
    }

    #[test]
    fn indeks_kolom_benar() {
        assert_eq!(indeks_kolom("A"), 0);
        assert_eq!(indeks_kolom("Z"), 25);
        assert_eq!(indeks_kolom("AA"), 26);
        assert_eq!(indeks_kolom("AL"), 37);
    }

    #[test]
    fn bukan_xlsx_ditolak() {
        let err = baca(b"bukan file excel").unwrap_err();
        assert!(err.contains("Form Laporan Mingguan"), "{err}");
    }

    #[test]
    fn sheet_wajib_tidak_ada_ditolak() {
        let mut wb = rust_xlsxwriter::Workbook::new();
        wb.add_worksheet().set_name("Lain").unwrap();
        let bytes = wb.save_to_buffer().unwrap();
        let err = baca(&bytes).unwrap_err();
        assert!(err.contains("Sheet 'Tahapan Sanimas' tidak ditemukan"), "{err}");
    }

    #[test]
    fn template_kosong_tidak_menghasilkan_lokasi() {
        let hasil = baca(super::super::TEMPLATE).unwrap();
        assert!(hasil.lokasi.is_empty(), "{:?}", hasil.lokasi);
        assert!(hasil.permasalahan.is_empty(), "{:?}", hasil.permasalahan);
    }

    #[test]
    fn form_dari_template_terbaca() {
        let ts = "Tahapan Sanimas";
        let tl = "Tahapan LPK";
        let gs = "Target Sanimas";
        let gl = "Target LPK";
        let pm = "Partisipan Masyarakat";
        let pr = "Permasalahan";
        let isian = vec![
            // Sanimas lokasi 1 (baris 12): tanggal serial, teks bulan Indonesia, teks m/d/yyyy.
            teks(ts, "C12", " Bogor "),
            teks(ts, "D12", "Ciomas"),
            teks(ts, "E12", "Pagelaran"),
            teks(ts, "F12", "Tahap 1"),
            angka(ts, "I12", 46211.0), // 2026-07-08
            teks(ts, "X12", "21 Agustus 2026"),
            teks(ts, "AI12", "4/18/2025"),
            teks(ts, "J12", "tgl-bln-th"),
            // Sanimas lokasi 2 (baris 13) tanpa tanggal, hanya target.
            teks(ts, "C13", "Bogor"),
            teks(ts, "D13", "Sukaraja"),
            teks(ts, "E13", "Cimandala"),
            teks(ts, "F13", "Tahap 1"),
            // Sanimas lokasi 3 (baris 14) tanpa tanggal dan target: tidak masuk.
            teks(ts, "C14", "Bogor"),
            teks(ts, "D14", "Citeureup"),
            teks(ts, "E14", "Tarikolot"),
            teks(ts, "F14", "Tahap 1"),
            // Target Sanimas: baris 10 = lokasi 1, baris 11 = lokasi 2.
            angka(gs, "G10", 36.0),
            angka(gs, "L10", 9786.5),
            teks(gs, "R10", "-"),
            teks(gs, "W10", "Daratan"),
            angka(gs, "AL10", 0.0),
            angka(gs, "H11", 35.0),
            // LPK lokasi 1 (baris 11), target di baris 10.
            teks(tl, "C11", "Bogor"),
            teks(tl, "D11", "Tanjung Sari"),
            teks(tl, "E11", "Tanjung Rasa"),
            teks(tl, "F11", "PP Daarul Muta'alimien"),
            teks(tl, "G11", "Tahap 1"),
            angka(tl, "I11", 46211.0),
            teks(tl, "AJ11", "3 Desember 2026"),
            angka(gl, "H10", 89.0),
            teks(gl, "K10", "Konvensional"),
            teks(gl, "T10", "Normal"),
            // Partisipan baris 9 dan 11 (baris 10 kosong dilewati).
            teks(pm, "A9", "Sanimas"),
            teks(pm, "C9", "Bogor"),
            teks(pm, "E9", "Pagelaran"),
            angka(pm, "P9", 31.0),
            teks(pm, "AJ9", "4"),
            teks(pm, "C11", "Cianjur"),
            teks(pm, "F11", "PP Masiakkul"),
            // Permasalahan baris 10: kolom A diabaikan.
            angka(pr, "A10", 1.0),
            teks(pr, "B10", "Bogor"),
            teks(pr, "G10", "Lahan belum siap"),
            teks(pr, "M10", "Proses"),
            // Baris 11 hanya berisi status: dilewati.
            teks(pr, "M11", "Proses / Selesai"),
        ];
        let bytes = isi_template(super::super::TEMPLATE, &isian).unwrap();
        let hasil = baca(&bytes).unwrap();

        assert_eq!(hasil.lokasi.len(), 3, "{:?}", hasil.lokasi);
        let k1 = store::kunci_lokasi("Bogor", "Ciomas", "Pagelaran", "Tahap 1", "");
        let l1 = cari(&hasil, "san", &k1);
        assert_eq!(l1.tahapan.get("1").map(String::as_str), Some("2026-07-08"));
        assert_eq!(l1.tahapan.get("16").map(String::as_str), Some("2026-08-21")); // X = tahap 16
        assert_eq!(l1.tahapan.get("27").map(String::as_str), Some("2025-04-18")); // AI = tahap 27
        assert_eq!(l1.tahapan.len(), 3);
        assert_eq!(l1.target.get("G").map(String::as_str), Some("36"));
        assert_eq!(l1.target.get("L").map(String::as_str), Some("9786.5"));
        assert_eq!(l1.target.get("W").map(String::as_str), Some("Daratan"));
        assert_eq!(l1.target.get("AL").map(String::as_str), Some("0"));
        assert!(!l1.target.contains_key("R"));
        // Kolom identitas Target (rumus ke Tahapan) tidak ikut.
        assert!(!l1.target.contains_key("A"));

        let k2 = store::kunci_lokasi("Bogor", "Sukaraja", "Cimandala", "Tahap 1", "");
        let l2 = cari(&hasil, "san", &k2);
        assert!(l2.tahapan.is_empty());
        assert_eq!(l2.target.get("H").map(String::as_str), Some("35"));

        let k3 = store::kunci_lokasi("Bogor", "Tanjung Sari", "Tanjung Rasa", "Tahap 1", "PP Daarul Muta'alimien");
        let l3 = cari(&hasil, "lpk", &k3);
        assert_eq!(l3.tahapan.get("1").map(String::as_str), Some("2026-07-08"));
        assert_eq!(l3.tahapan.get("28").map(String::as_str), Some("2026-12-03"));
        assert_eq!(l3.target.get("H").map(String::as_str), Some("89"));
        assert_eq!(l3.target.get("K").map(String::as_str), Some("Konvensional"));
        assert_eq!(l3.target.get("T").map(String::as_str), Some("Normal"));

        let p1 = &hasil.partisipan[0];
        assert_eq!(p1.get("A").map(String::as_str), Some("Sanimas"));
        assert_eq!(p1.get("E").map(String::as_str), Some("Pagelaran"));
        assert_eq!(p1.get("P").map(String::as_str), Some("31"));
        assert_eq!(p1.get("AJ").map(String::as_str), Some("4"));
        assert!(hasil.partisipan.iter().any(|p| p.get("C").map(String::as_str) == Some("Cianjur")));

        assert_eq!(hasil.permasalahan.len(), 1, "{:?}", hasil.permasalahan);
        let m = &hasil.permasalahan[0];
        assert!(!m.contains_key("A"));
        assert_eq!(m.get("B").map(String::as_str), Some("Bogor"));
        assert_eq!(m.get("G").map(String::as_str), Some("Lahan belum siap"));
        assert_eq!(m.get("M").map(String::as_str), Some("Proses"));
    }

    /// Workbook kecil dengan rumus ber-cache: target rumus diabaikan, partisipan rumus memakai cache,
    /// sel tanggal ber-format tanggal terbaca.
    #[test]
    fn rumus_dan_sel_tanggal() {
        use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Workbook};
        let mut wb = Workbook::new();
        let tgl = Format::new().set_num_format("dd/mm/yyyy");
        {
            let ws = wb.add_worksheet().set_name("Tahapan Sanimas").unwrap();
            ws.write_string(11, 2, "Bekasi").unwrap();
            ws.write_string(11, 3, "Cibarusah").unwrap();
            ws.write_string(11, 4, "Cibarusah Jaya").unwrap();
            ws.write_string(11, 5, "Tahap 2").unwrap();
            let d = ExcelDateTime::from_ymd(2026, 9, 7).unwrap();
            ws.write_datetime_with_format(11, 8, &d, &tgl).unwrap();
        }
        {
            let ws = wb.add_worksheet().set_name("Target Sanimas").unwrap();
            ws.write_formula(9, 6, Formula::new("=1+1").set_result("2")).unwrap(); // G10 rumus
            ws.write_number(9, 7, 25.0).unwrap(); // H10 konstanta
        }
        wb.add_worksheet().set_name("Tahapan LPK").unwrap();
        wb.add_worksheet().set_name("Target LPK").unwrap();
        {
            let ws = wb.add_worksheet().set_name("Partisipan Masyarakat").unwrap();
            ws.write_formula(8, 0, Formula::new("=\"Sanimas\"").set_result("Sanimas")).unwrap();
            ws.write_string(8, 2, "Bekasi").unwrap();
        }
        wb.add_worksheet().set_name("Permasalahan").unwrap();
        let bytes = wb.save_to_buffer().unwrap();

        let hasil = baca(&bytes).unwrap();
        assert_eq!(hasil.lokasi.len(), 1);
        let (jenis, kunci, data) = &hasil.lokasi[0];
        assert_eq!(jenis, "san");
        assert_eq!(kunci, &store::kunci_lokasi("Bekasi", "Cibarusah", "Cibarusah Jaya", "Tahap 2", ""));
        assert_eq!(data.tahapan.get("1").map(String::as_str), Some("2026-09-07"));
        assert!(!data.target.contains_key("G"), "{:?}", data.target);
        assert_eq!(data.target.get("H").map(String::as_str), Some("25"));
        assert_eq!(hasil.partisipan.len(), 1);
        assert_eq!(hasil.partisipan[0].get("A").map(String::as_str), Some("Sanimas"));
    }

    /// Form lama nyata (tidak disimpan di repo); dilewati bila file tidak ada.
    #[test]
    fn form_lama_nyata() {
        let path = "/root/.claude/uploads/ed2718b6-4e6a-595b-9618-806d016befd8/98f3f357-Form_Laporan_Mingguan_Ganthi_I_November_Format.xlsx";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let hasil = baca(&bytes).unwrap();
        let san = hasil.lokasi.iter().filter(|(j, _, _)| j == "san").count();
        let lpk = hasil.lokasi.iter().filter(|(j, _, _)| j == "lpk").count();
        assert!(san >= 10, "san {san}");
        assert_eq!(lpk, 2);

        let l = cari(&hasil, "san", &store::kunci_lokasi("Bogor", "Ciomas", "Pagelaran", "Tahap 1", ""));
        assert_eq!(l.tahapan.get("1").map(String::as_str), Some("2026-07-08"));
        assert_eq!(l.tahapan.get("16").map(String::as_str), Some("2026-08-21")); // X12 teks
        assert_eq!(l.target.get("G").map(String::as_str), Some("36"));
        assert_eq!(l.target.get("L").map(String::as_str), Some("9786"));
        assert_eq!(l.target.get("W").map(String::as_str), Some("Daratan"));
        assert!(!l.target.contains_key("R")); // "-"

        let k = store::kunci_lokasi("Bogor", "Tanjung Sari", "Tanjung Rasa", "Tahap 1", "PP Daarul Muta'alimien");
        let l = cari(&hasil, "lpk", &k);
        assert_eq!(l.tahapan.get("1").map(String::as_str), Some("2026-07-08"));
        assert_eq!(l.target.get("H").map(String::as_str), Some("89"));
        assert_eq!(l.target.get("N").map(String::as_str), Some("Ya"));

        // Partisipan: nilai cache rumus (kegiatan) ikut terbaca.
        let p = &hasil.partisipan[0];
        assert_eq!(p.get("A").map(String::as_str), Some("Sanimas"));
        assert_eq!(p.get("E").map(String::as_str), Some("Pagelaran"));
        assert_eq!(p.get("P").map(String::as_str), Some("31"));
        // Permasalahan di file ini hanya sisa contoh (tanggal dan status tanpa uraian).
        assert!(hasil.permasalahan.is_empty(), "{:?}", hasil.permasalahan);

        for (_, _, d) in &hasil.lokasi {
            for t in d.tahapan.values() {
                assert!(NaiveDate::parse_from_str(t, "%Y-%m-%d").is_ok(), "{t}");
            }
        }
    }
}
