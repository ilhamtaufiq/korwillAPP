//! Isian Form Mingguan dari data tersimpan (`store::FormData`): tanggal tahapan, target, partisipan,
//! permasalahan; ditambah RPDB per tahap dari Lapming untuk sheet Keuangan Sanimas.
//!
//! Sel yang di template berisi rumus tidak pernah ditimpa: isian ke sel itu dilewati dan dicatat sebagai peringatan.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::NaiveDate;
use regex::Regex;

use super::isi::{self, Isian, Nilai};
use super::store::{self, FormData};
use crate::lapming::{calc::DataPokmas, fmt::norm_nama, model::Row};

/// Baris data Partisipan Masyarakat di template: 9..=330 (baris terakhir yang masih ber-style).
pub const PARTISIPAN_AWAL: usize = 9;
pub const KAPASITAS_PARTISIPAN: usize = 330 - PARTISIPAN_AWAL + 1;
/// Baris data Permasalahan di template: 10..=1000.
pub const PERMASALAHAN_AWAL: usize = 10;
pub const KAPASITAS_PERMASALAHAN: usize = 1000 - PERMASALAHAN_AWAL + 1;

/// Kolom (tanggal, nilai Rp.1000) RPDB-1..6 di sheet Keuangan Sanimas (header baris 8-10).
const RPDB_KOLOM: [(&str, &str); 6] = [("O", "P"), ("W", "X"), ("AE", "AF"), ("AM", "AN"), ("AU", "AV"), ("BC", "BD")];

/// Kolom Partisipan Masyarakat yang berupa teks (identitas lokasi); sisanya jumlah orang.
const PARTISIPAN_TEKS: [&str; 9] = ["A", "B", "C", "D", "E", "F", "G", "H", "I"];
/// Kolom tanggal di sheet Permasalahan.
const PERMASALAHAN_TANGGAL: [&str; 2] = ["H", "K"];

const SHEET_DIPERIKSA: [&str; 7] = [
    "Tahapan Sanimas",
    "Tahapan LPK",
    "Target Sanimas",
    "Target LPK",
    "Partisipan Masyarakat",
    "Permasalahan",
    "Keuangan Sanimas",
];

/// Nomor seri tanggal Excel (hari sejak 1899-12-30).
pub fn serial(d: NaiveDate) -> f64 {
    (d - NaiveDate::from_ymd_opt(1899, 12, 30).unwrap()).num_days() as f64
}

/// Tanggal dari teks "YYYY-MM-DD" (format simpan); "DD-MM-YYYY" dan "DD/MM/YYYY" juga diterima.
pub fn tanggal(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    ["%Y-%m-%d", "%d-%m-%Y", "%d/%m/%Y"].iter().find_map(|f| NaiveDate::parse_from_str(s, f).ok())
}

/// Teks berbentuk angka menjadi angka. Diterima: "12", "-3", "12.5", "12,5", dan ribuan bertitik "1.250.000"
/// atau "1.250,5". Selain itu tetap teks.
pub fn angka_dari_teks(s: &str) -> Option<f64> {
    let s = s.trim();
    let ribuan = Regex::new(r"^-?\d{1,3}(\.\d{3})+(,\d+)?$").unwrap();
    let koma = Regex::new(r"^-?\d+(,\d+)?$").unwrap();
    let titik = Regex::new(r"^-?\d+(\.\d+)?$").unwrap();
    let baku = if ribuan.is_match(s) {
        s.replace('.', "").replace(',', ".")
    } else if koma.is_match(s) {
        s.replace(',', ".")
    } else if titik.is_match(s) {
        s.to_string()
    } else {
        return None;
    };
    baku.parse().ok()
}

fn angka_atau_teks(s: &str) -> Nilai {
    match angka_dari_teks(s) {
        Some(a) => Nilai::Angka(a),
        None => Nilai::Teks(s.trim().to_string()),
    }
}

fn kunci_row(x: &Row) -> String {
    let nama = if x.lpk { x.nama.as_str() } else { "" };
    store::kunci_lokasi(
        x.kab.as_deref().unwrap_or(""),
        x.kec.as_deref().unwrap_or(""),
        &x.desa,
        x.tahap.as_deref().unwrap_or(""),
        nama,
    )
}

/// Kolom realisasi penggunaan dana (material, upah, BOP) per RPDB-1..6. Alat tidak ada di Lapming.
/// Letaknya sama di Keuangan Sanimas dan Keuangan LPK Swakelola.
const REALISASI_KOLOM: [(&str, &str, &str); 6] =
    [("Q", "S", "T"), ("Y", "AA", "AB"), ("AG", "AI", "AJ"), ("AO", "AQ", "AR"), ("AW", "AY", "AZ"), ("BE", "BG", "BH")];

/// Nomor tahap (teks, 1-based) untuk kode kolom tahapan, mis. "X" -> "16" di Sanimas.
fn no_tahap(kol: &[(&str, &str)], kode: &str) -> String {
    kol.iter().position(|(k, _)| *k == kode).map(|i| (i + 1).to_string()).unwrap_or_default()
}

/// Penggunaan dana mingguan (material, upah, BOP dalam rupiah) dijumlahkan per RPDB. Minggu masuk ke RPDB
/// terakhir yang tanggalnya tidak melewati akhir minggu itu; minggu sebelum RPDB pertama masuk ke RPDB-1.
fn realisasi_per_rpdb(x: &Row, riwayat: &[(u32, Vec<Row>)], tanggal: &[Option<NaiveDate>]) -> Vec<[f64; 3]> {
    let mut hasil = vec![[0.0; 3]; REALISASI_KOLOM.len()];
    for (w, rows) in riwayat {
        let Some(y) = rows.iter().find(|y| y.lpk == x.lpk && y.no == x.no) else { continue };
        let akhir = super::kurva::akhir_minggu(*w);
        let tahap = tanggal
            .iter()
            .enumerate()
            .filter(|(_, d)| d.is_some_and(|d| d <= akhir))
            .map(|(t, _)| t)
            .last()
            .unwrap_or(0)
            .min(REALISASI_KOLOM.len() - 1);
        hasil[tahap][0] += y.u;
        hasil[tahap][1] += y.s;
        hasil[tahap][2] += y.w;
    }
    hasil
}

/// Keuangan Sanimas dan LPK Swakelola, serta penyaluran dana di Fisik & HOK.
#[allow(clippy::too_many_arguments)]
fn keuangan(
    out: &mut Vec<Isian>,
    peringatan: &mut Vec<String>,
    san: &[&Row],
    lpk: &[&Row],
    form: &FormData,
    pokmas: &DataPokmas,
    riwayat: &[(u32, Vec<Row>)],
) {
    struct Bagian<'a> {
        jenis: &'a str,
        daftar: &'a [&'a Row],
        keu: &'a str,
        fisik: &'a str,
        tahap: &'a [(&'a str, &'a str)],
        kode_t1: &'a str,
        kode_t2: &'a str,
        /// (kolom tanggal, kolom nilai bila isian) untuk Tahap I dan II di sheet Keuangan.
        t1: (&'a str, Option<&'a str>),
        t2: (&'a str, Option<&'a str>),
        /// Kolom Tahap I dan II di Fisik & HOK.
        fisik_t: (&'a str, &'a str),
        /// Kolom pagu di sheet Keuangan bila berupa isian (LPK), None bila rumus (Sanimas).
        pagu: Option<&'a str>,
    }
    let bagian = [
        Bagian {
            jenis: "san", daftar: san, keu: "Keuangan Sanimas", fisik: "Fisik & HOK Sanimas", tahap: &store::TAHAP_SAN,
            kode_t1: "X", kode_t2: "AC", t1: ("H", None), t2: ("J", Some("K")), fisik_t: ("M", "N"), pagu: None,
        },
        Bagian {
            jenis: "lpk", daftar: lpk, keu: "Keuangan LPK Swakelola", fisik: "Fisik & HOK LPK", tahap: &store::TAHAP_LPK,
            kode_t1: "Y", kode_t2: "AC", t1: ("I", Some("J")), t2: ("K", Some("L")), fisik_t: ("N", "O"), pagu: Some("H"),
        },
    ];
    let tulis = |out: &mut Vec<Isian>, sheet: &str, sel: String, n: Nilai| out.push((sheet.to_string(), sel, n));
    for b in bagian {
        let no_t1 = no_tahap(b.tahap, b.kode_t1);
        let no_t2 = no_tahap(b.tahap, b.kode_t2);
        for (i, x) in b.daftar.iter().enumerate() {
            let rk = 11 + i;
            let rf = 13 + i;
            if let Some(kol) = b.pagu {
                tulis(out, b.keu, format!("{kol}{rk}"), Nilai::Angka(x.pagu / 1000.0));
            }
            if b.jenis == "san" {
                // Template asli tidak punya rumus Jumlah RPDB-3; tanpa ini TOTAL tidak menghitung RPDB-3.
                tulis(out, b.keu, format!("AK{rk}"), Nilai::Rumus(format!("SUM(AG{rk}:AJ{rk})")));
            }

            // RPDB per tahap dari halaman Lapming.
            let rpdb = pokmas.rpdb.get(&norm_nama(&x.nama));
            if x.rpdb_t.len() > RPDB_KOLOM.len() {
                peringatan.push(format!("RPDB '{}' punya {} tahap; hanya {} yang muat di form", x.nama, x.rpdb_t.len(), RPDB_KOLOM.len()));
            }
            let mut tgl_rpdb: Vec<Option<NaiveDate>> = Vec::new();
            for (t, ((kol_tgl, kol_nilai), jumlah)) in RPDB_KOLOM.iter().zip(&x.rpdb_t).enumerate() {
                let tgl = rpdb.and_then(|v| v.get(t)).and_then(|v| v.tanggal);
                tgl_rpdb.push(if jumlah.is_some() { tgl } else { None });
                let Some(jumlah) = jumlah else { continue };
                tulis(out, b.keu, format!("{kol_nilai}{rk}"), Nilai::Angka(*jumlah as f64 / 1000.0));
                if let Some(d) = tgl {
                    tulis(out, b.keu, format!("{kol_tgl}{rk}"), Nilai::Tanggal(serial(d)));
                }
            }

            // Realisasi penggunaan dana per RPDB dari data mingguan Lapming.
            for (t, [material, upah, bop]) in realisasi_per_rpdb(x, riwayat, &tgl_rpdb).into_iter().enumerate() {
                if material == 0.0 && upah == 0.0 && bop == 0.0 {
                    continue;
                }
                let (km, ku, kb) = REALISASI_KOLOM[t];
                tulis(out, b.keu, format!("{km}{rk}"), Nilai::Angka(material / 1000.0));
                tulis(out, b.keu, format!("{ku}{rk}"), Nilai::Angka(upah / 1000.0));
                tulis(out, b.keu, format!("{kb}{rk}"), Nilai::Angka(bop / 1000.0));
            }

            // Penyaluran Tahap I/II: tanggal dari isian tahapan; Tahap I juga dianggap cair bila RPDB-1 sudah ada.
            let isian = form.lokasi.get(&(b.jenis.to_string(), kunci_row(x)));
            let tgl_t = |no: &str| isian.and_then(|d| d.tahapan.get(no)).and_then(|v| tanggal(v));
            let tgl_t1 = tgl_t(&no_t1);
            let tgl_t2 = tgl_t(&no_t2);
            let cair_t1 = tgl_t1.is_some() || x.rpdb_t.first().is_some_and(|v| v.is_some());
            let cair_t2 = tgl_t2.is_some();
            if let Some(d) = tgl_t1 {
                tulis(out, b.keu, format!("{}{rk}", b.t1.0), Nilai::Tanggal(serial(d)));
            }
            if let Some(d) = tgl_t2 {
                tulis(out, b.keu, format!("{}{rk}", b.t2.0), Nilai::Tanggal(serial(d)));
            }
            if cair_t1 {
                tulis(out, b.fisik, format!("{}{rf}", b.fisik_t.0), Nilai::Angka(x.pagu * 0.7 / 1000.0));
                if let Some(kol) = b.t1.1 {
                    tulis(out, b.keu, format!("{kol}{rk}"), Nilai::Angka(x.pagu * 0.7 / 1000.0));
                }
            }
            if cair_t2 {
                tulis(out, b.fisik, format!("{}{rf}", b.fisik_t.1), Nilai::Angka(x.pagu * 0.3 / 1000.0));
                if let Some(kol) = b.t2.1 {
                    tulis(out, b.keu, format!("{kol}{rk}"), Nilai::Angka(x.pagu * 0.3 / 1000.0));
                }
            }
        }
    }
}

/// Isian dari data tersimpan dan RPDB, beserta peringatan (isian yang dilewati).
pub fn susun(
    rows: &[Row],
    form: &FormData,
    pokmas: &DataPokmas,
    template: &[u8],
    riwayat: &[(u32, Vec<Row>)],
) -> Result<(Vec<Isian>, Vec<String>), String> {
    if form.partisipan.len() > KAPASITAS_PARTISIPAN {
        return Err(format!(
            "Jumlah baris Partisipan Masyarakat {} melebihi kapasitas form ({KAPASITAS_PARTISIPAN})",
            form.partisipan.len()
        ));
    }
    if form.permasalahan.len() > KAPASITAS_PERMASALAHAN {
        return Err(format!(
            "Jumlah baris Permasalahan {} melebihi kapasitas form ({KAPASITAS_PERMASALAHAN})",
            form.permasalahan.len()
        ));
    }

    let mut out: Vec<Isian> = Vec::new();
    let mut peringatan: Vec<String> = Vec::new();
    let tulis = |out: &mut Vec<Isian>, sheet: &str, sel: String, n: Nilai| out.push((sheet.to_string(), sel, n));

    // Satu isian tersimpan hanya ditulis sekali, di baris lokasi pertama yang cocok.
    let mut sudah: HashSet<(&str, String)> = HashSet::new();
    let san: Vec<&Row> = rows.iter().filter(|x| !x.lpk).collect();
    let lpk: Vec<&Row> = rows.iter().filter(|x| x.lpk).collect();
    let bagian: [(&str, &Vec<&Row>, usize, usize, &str, &str, &[(&str, &str)], &[(&str, &str)]); 2] = [
        ("san", &san, 12, 10, "Tahapan Sanimas", "Target Sanimas", &store::TAHAP_SAN, &store::TARGET_SAN),
        ("lpk", &lpk, 11, 10, "Tahapan LPK", "Target LPK", &store::TAHAP_LPK, &store::TARGET_LPK),
    ];
    for (jenis, daftar, awal_tahap, awal_target, sh_tahap, sh_target, kol_tahap, kol_target) in bagian {
        let target_sah: HashSet<&str> = kol_target.iter().map(|(k, _)| *k).collect();
        for (i, x) in daftar.iter().enumerate() {
            let kunci = kunci_row(x);
            let Some(data) = form.lokasi.get(&(jenis.to_string(), kunci.clone())) else { continue };
            if !sudah.insert((jenis, kunci.clone())) {
                peringatan.push(format!("Lokasi {jenis} '{kunci}' muncul lebih dari sekali; isian tersimpan hanya ditulis di baris pertama"));
                continue;
            }
            let t = awal_tahap + i;
            for (no, tgl) in &data.tahapan {
                if tgl.trim().is_empty() {
                    continue;
                }
                let kol = no.parse::<usize>().ok().filter(|n| (1..=kol_tahap.len()).contains(n)).map(|n| kol_tahap[n - 1].0);
                match (kol, tanggal(tgl)) {
                    (Some(k), Some(d)) => tulis(&mut out, sh_tahap, format!("{k}{t}"), Nilai::Tanggal(serial(d))),
                    _ => peringatan.push(format!("Tahap {no} = '{tgl}' untuk lokasi '{kunci}' tidak valid, dilewati")),
                }
            }
            let g = awal_target + i;
            for (kol, v) in &data.target {
                if v.trim().is_empty() {
                    continue;
                }
                if !target_sah.contains(kol.as_str()) {
                    peringatan.push(format!("Kolom target {jenis} '{kol}' tidak dikenal, dilewati"));
                    continue;
                }
                tulis(&mut out, sh_target, format!("{kol}{g}"), angka_atau_teks(v));
            }
        }
    }

    // Keuangan dan penyaluran dana: sama seperti laporan Lapming (M1 sampai minggu terpilih), beda format.
    keuangan(&mut out, &mut peringatan, &san, &lpk, form, pokmas, riwayat);

    let tulis_baris = |out: &mut Vec<Isian>, peringatan: &mut Vec<String>, sheet: &str, baris: usize, peta: &BTreeMap<String, String>, kolom: &[(&str, &str)], nilai: &dyn Fn(&str, &str) -> Nilai| {
        let sah: HashSet<&str> = kolom.iter().map(|(k, _)| *k).collect();
        for (kol, v) in peta {
            if v.trim().is_empty() {
                continue;
            }
            if !sah.contains(kol.as_str()) {
                peringatan.push(format!("{sheet}: kolom '{kol}' tidak dikenal, dilewati"));
                continue;
            }
            out.push((sheet.to_string(), format!("{kol}{baris}"), nilai(kol, v)));
        }
    };

    for (k, peta) in form.partisipan.iter().enumerate() {
        tulis_baris(&mut out, &mut peringatan, "Partisipan Masyarakat", PARTISIPAN_AWAL + k, peta, &store::PARTISIPAN_KOLOM, &|kol, v| {
            if PARTISIPAN_TEKS.contains(&kol) {
                Nilai::Teks(v.trim().to_string())
            } else {
                angka_atau_teks(v)
            }
        });
    }
    for (k, peta) in form.permasalahan.iter().enumerate() {
        let baris = PERMASALAHAN_AWAL + k;
        tulis(&mut out, "Permasalahan", format!("A{baris}"), Nilai::Angka((k + 1) as f64));
        tulis_baris(&mut out, &mut peringatan, "Permasalahan", baris, peta, &store::PERMASALAHAN_KOLOM, &|kol, v| {
            match (PERMASALAHAN_TANGGAL.contains(&kol), tanggal(v)) {
                (true, Some(d)) => Nilai::Tanggal(serial(d)),
                _ => Nilai::Teks(v.trim().to_string()),
            }
        });
    }

    // Jangan menimpa rumus template.
    let mut rumus: HashMap<&str, HashSet<String>> = HashMap::new();
    for sh in SHEET_DIPERIKSA {
        rumus.insert(sh, isi::sel_rumus(template, sh)?);
    }
    out.retain(|(sheet, sel, _)| {
        let ada = rumus.get(sheet.as_str()).is_some_and(|s| s.contains(sel));
        if ada {
            peringatan.push(format!("{sheet}!{sel} berisi rumus di template, isian dilewati"));
        }
        !ada
    });
    Ok((out, peringatan))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angka_dari_teks_mengenali_format_umum() {
        assert_eq!(angka_dari_teks("12"), Some(12.0));
        assert_eq!(angka_dari_teks(" 12.5 "), Some(12.5));
        assert_eq!(angka_dari_teks("12,5"), Some(12.5));
        assert_eq!(angka_dari_teks("1.250.000"), Some(1_250_000.0));
        assert_eq!(angka_dari_teks("-3"), Some(-3.0));
        assert_eq!(angka_dari_teks("Daratan"), None);
        assert_eq!(angka_dari_teks("12 unit"), None);
        assert_eq!(angka_dari_teks(""), None);
    }

    #[test]
    fn serial_tanggal_excel() {
        assert_eq!(serial(NaiveDate::from_ymd_opt(1900, 3, 1).unwrap()), 61.0);
        assert_eq!(serial(tanggal("2026-03-15").unwrap()), 46096.0);
        assert_eq!(tanggal("15-03-2026"), tanggal("2026-03-15"));
        assert_eq!(tanggal("bukan"), None);
    }
}
