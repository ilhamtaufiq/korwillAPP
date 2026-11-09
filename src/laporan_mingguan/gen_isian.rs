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

/// Baris awal blok lokasi di KURVA-S (rencana; realisasi di +2). Blok 58 dan 84 adalah ringkasan, dilewati.
const KURVA_BLOK: [u32; 13] = [13, 18, 23, 28, 33, 38, 43, 48, 53, 64, 69, 74, 79];
/// Kolom periode KURVA-S: I..=AQ (35 periode mingguan).
const KURVA_PERIODE: usize = 35;
const KURVA_KOLOM_AWAL: u32 = 9;

fn huruf_kolom(mut c: u32) -> String {
    let mut out = String::new();
    while c > 0 {
        let sisa = (c - 1) % 26;
        out.insert(0, (b'A' + sisa as u8) as char);
        c = (c - 1) / 26;
    }
    out
}

/// Akhir periode KURVA-S ke-p (0-based): 19 Maret 2026, lalu tiap 7 hari.
fn akhir_periode(p: usize) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 3, 19).unwrap() + chrono::Duration::days(7 * p as i64)
}

/// Akhir minggu Lapming ke-w: minggu 1 berakhir 7 September 2026.
fn akhir_minggu(w: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 7).unwrap() + chrono::Duration::days(7 * (w as i64 - 1))
}

/// Nilai kumulatif (rencana, realisasi) satu lokasi pada akhir periode `p`, dari minggu Lapming terakhir yang selesai.
fn kumulatif_pada(riwayat: &[(u32, Vec<Row>)], lpk: bool, no: &str, p: usize) -> (f64, f64) {
    let batas = akhir_periode(p);
    let mut terbaru: Option<(u32, f64, f64)> = None;
    for (w, rows) in riwayat {
        if akhir_minggu(*w) > batas {
            continue;
        }
        if let Some(x) = rows.iter().find(|x| x.lpk == lpk && x.no == no) {
            if terbaru.map_or(true, |(tw, _, _)| *w > tw) {
                terbaru = Some((*w, x.ah.unwrap_or(0.0), x.ag));
            }
        }
    }
    terbaru.map(|(_, a, b)| (a, b)).unwrap_or((0.0, 0.0))
}

/// KURVA-S: tiap lokasi mengisi satu blok. Rumus di template menjumlahkan selisih per periode,
/// jadi yang ditulis adalah selisih kumulatif antar periode.
fn kurva_s(rows: &[Row], riwayat: &[(u32, Vec<Row>)]) -> (Vec<Isian>, Vec<String>) {
    let mut out = Vec::new();
    let mut peringatan = Vec::new();
    if rows.len() > KURVA_BLOK.len() {
        peringatan.push(format!(
            "KURVA-S hanya muat {} lokasi; {} lokasi lain tidak ditulis",
            KURVA_BLOK.len(),
            rows.len() - KURVA_BLOK.len()
        ));
    }
    for (i, x) in rows.iter().take(KURVA_BLOK.len()).enumerate() {
        let awal = KURVA_BLOK[i];
        let mut rencana_kum = Vec::with_capacity(KURVA_PERIODE);
        let mut realisasi_kum = Vec::with_capacity(KURVA_PERIODE);
        for p in 0..KURVA_PERIODE {
            let (r, s) = kumulatif_pada(riwayat, x.lpk, &x.no, p);
            rencana_kum.push(r);
            realisasi_kum.push(s);
        }
        let selisih = |kum: &[f64]| -> Vec<f64> {
            kum.iter().enumerate().map(|(p, v)| if p == 0 { *v } else { v - kum[p - 1] }).collect()
        };
        let tulis_blok = |out: &mut Vec<Isian>, baris: u32, nilai: &[f64]| {
            for (p, v) in nilai.iter().enumerate() {
                let kol = huruf_kolom(KURVA_KOLOM_AWAL + p as u32);
                out.push(("KURVA-S".into(), format!("{kol}{baris}"), Nilai::Angka(*v)));
            }
        };
        out.push(("KURVA-S".into(), format!("A{awal}"), Nilai::Angka((i + 1) as f64)));
        out.push(("KURVA-S".into(), format!("B{awal}"), Nilai::Teks(x.kab.clone().unwrap_or_default())));
        out.push(("KURVA-S".into(), format!("C{awal}"), Nilai::Teks(x.kec.clone().unwrap_or_default())));
        out.push(("KURVA-S".into(), format!("D{awal}"), Nilai::Teks(x.desa.clone())));
        out.push(("KURVA-S".into(), format!("E{awal}"), Nilai::Teks(x.nama.clone())));
        tulis_blok(&mut out, awal, &selisih(&rencana_kum));
        tulis_blok(&mut out, awal + 2, &selisih(&realisasi_kum));
    }
    (out, peringatan)
}

/// Isian dari data tersimpan dan RPDB, beserta peringatan (isian yang dilewati).
pub fn susun(rows: &[Row], form: &FormData, pokmas: &DataPokmas, template: &[u8], riwayat: &[(u32, Vec<Row>)]) -> Result<(Vec<Isian>, Vec<String>), String> {
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

    // Keuangan Sanimas: RPDB per tahap (Rp.1000) dan tanggalnya, baris 11+i.
    for (i, x) in san.iter().enumerate() {
        let r = 11 + i;
        let tgl = pokmas.rpdb.get(&norm_nama(&x.nama));
        if x.rpdb_t.len() > RPDB_KOLOM.len() {
            peringatan.push(format!("RPDB '{}' punya {} tahap; hanya {} yang muat di form", x.nama, x.rpdb_t.len(), RPDB_KOLOM.len()));
        }
        for (t, ((kol_tgl, kol_nilai), jumlah)) in RPDB_KOLOM.iter().zip(&x.rpdb_t).enumerate() {
            let Some(jumlah) = jumlah else { continue };
            tulis(&mut out, "Keuangan Sanimas", format!("{kol_nilai}{r}"), Nilai::Angka(*jumlah as f64 / 1000.0));
            if let Some(d) = tgl.and_then(|v| v.get(t)).and_then(|v| v.tanggal) {
                tulis(&mut out, "Keuangan Sanimas", format!("{kol_tgl}{r}"), Nilai::Tanggal(serial(d)));
            }
        }
    }

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
    let (kurva, peringatan_kurva) = kurva_s(rows, riwayat);
    out.extend(kurva);
    peringatan.extend(peringatan_kurva);
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
