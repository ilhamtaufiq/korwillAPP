//! Membaca workbook rekap Lapming (sheet M1, M2, ...). Kolom dipetakan lewat teks header baris 4 dan 6,
//! karena letak kolom berbeda antar sheet dan antar file (Sanimas dan LPK).

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{open_workbook_auto_from_rs, Data, Range, Reader};
use chrono::{Duration, NaiveDate};
use regex::Regex;

use super::fmt::{short, tanpa_pokmas};
use super::model::{Book, Row};

/// (kolom, nama yang ditampilkan) untuk data wajib.
const WAJIB: [(&str, &str); 4] = [
    ("AH", "target kurva S"),
    ("AG", "fisik kumulatif"),
    ("AD", "keuangan kumulatif"),
    ("AA", "dana kumulatif"),
];

/// Posisi kolom (1-based, seperti openpyxl) untuk setiap isian.
#[derive(Default, Debug)]
struct Kolom {
    desa: u32,
    nama: u32,
    korfas: u32,
    pagu: u32,
    mulai: u32,
    selesai: u32,
    tahap: Option<u32>,
    kab: Option<u32>,
    kec: Option<u32>,
    no: u32,
    p: u32,
    q: u32,
    r: u32,
    s: u32,
    u: u32,
    w: u32,
    y: u32,
    z: u32,
    aa: u32,
    ab: u32,
    ac: u32,
    ad: u32,
    ae: u32,
    af: u32,
    ag: u32,
    ah: u32,
}

static KOSONG: Data = Data::Empty;

/// Akses sel dengan koordinat 1-based seperti openpyxl, di atas Range calamine.
struct Grid<'a> {
    range: &'a Range<Data>,
}

impl<'a> Grid<'a> {
    fn get(&self, row: u32, col: u32) -> &'a Data {
        if row == 0 || col == 0 {
            return &KOSONG;
        }
        self.range.get_value((row - 1, col - 1)).unwrap_or(&KOSONG)
    }

    fn max_row(&self) -> u32 {
        self.range.end().map(|e| e.0 + 1).unwrap_or(0)
    }

    fn max_col(&self) -> u32 {
        self.range.end().map(|e| e.1 + 1).unwrap_or(0)
    }
}

fn is_num(d: &Data) -> bool {
    matches!(d, Data::Int(_) | Data::Float(_))
}

fn as_f64(d: &Data) -> Option<f64> {
    match d {
        Data::Int(i) => Some(*i as f64),
        Data::Float(f) => Some(*f),
        _ => None,
    }
}

/// Teks sel seperti `str(value)` di Python untuk nilai yang kita pakai (kosong -> "").
fn cell_text(d: &Data) -> String {
    match d {
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            if *f == f.trunc() && f.abs() < 1e15 {
                format!("{:.1}", f)
            } else {
                f.to_string()
            }
        }
        Data::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Data::DateTime(dt) => dt.as_f64().to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

fn serial_ke_tanggal(serial: f64) -> Option<NaiveDate> {
    let basis = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    basis.checked_add_signed(Duration::days(serial.floor() as i64))
}

/// Sel tanggal, "21 September 2026", atau "3/9/2026" menjadi tanggal; selain itu None.
fn tanggal(d: &Data, tahun: i32) -> Option<NaiveDate> {
    match d {
        Data::DateTime(dt) => serial_ke_tanggal(dt.as_f64()),
        Data::DateTimeIso(s) => NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok(),
        Data::String(s) => tanggal_teks(s, tahun),
        _ => None,
    }
}

fn bulan_angka(nama: &str) -> Option<u32> {
    Some(match nama {
        "januari" | "jan" => 1,
        "februari" | "feb" => 2,
        "maret" | "mar" => 3,
        "april" | "apr" => 4,
        "mei" => 5,
        "juni" | "jun" => 6,
        "juli" | "jul" => 7,
        "agustus" | "agu" | "agt" => 8,
        "september" | "sep" | "sept" => 9,
        "oktober" | "okt" => 10,
        "november" | "nov" => 11,
        "desember" | "des" => 12,
        _ => return None,
    })
}

fn tanggal_teks(s: &str, tahun: i32) -> Option<NaiveDate> {
    let s = s.trim().to_lowercase();
    let re_num = Regex::new(r"^(\d{1,2})[/-](\d{1,2})[/-](\d{2,4})$").unwrap();
    if let Some(m) = re_num.captures(&s) {
        let d: u32 = m[1].parse().ok()?;
        let mo: u32 = m[2].parse().ok()?;
        let mut y: i32 = m[3].parse().ok()?;
        if y < 100 {
            y += 2000;
        }
        return NaiveDate::from_ymd_opt(y, mo, d);
    }
    let re_nama = Regex::new(r"^(\d{1,2})\s*([a-z]+)\s*(\d{4})?$").unwrap();
    if let Some(m) = re_nama.captures(&s) {
        let mo = bulan_angka(&m[2])?;
        let d: u32 = m[1].parse().ok()?;
        let y = m.get(3).and_then(|x| x.as_str().parse().ok()).unwrap_or(tahun);
        return NaiveDate::from_ymd_opt(y, mo, d);
    }
    None
}

fn kolom(g: &Grid) -> Result<Kolom, String> {
    let maxc = g.max_col();
    let mut h4: BTreeMap<u32, String> = BTreeMap::new();
    let mut h6: BTreeMap<u32, String> = BTreeMap::new();
    for c in 1..=maxc {
        let d4 = g.get(4, c);
        if !matches!(d4, Data::Empty) {
            h4.insert(c, cell_text(d4).to_lowercase().replace('\n', " "));
        }
        let d6 = g.get(6, c);
        if !matches!(d6, Data::Empty) {
            h6.insert(c, cell_text(d6).trim().to_lowercase());
        }
    }

    let cari = |d: &BTreeMap<u32, String>, kata: &[&str], excl: &[&str]| -> Option<u32> {
        d.iter()
            .find(|(_, t)| kata.iter().all(|k| t.contains(k)) && !excl.iter().any(|e| t.contains(e)))
            .map(|(c, _)| *c)
    };

    let desa = cari(&h4, &["desa"], &[]);
    let nama = cari(&h4, &["nama"], &["korfas", "korwil", "tfl"]);
    let korfas = cari(&h4, &["korfas"], &[]).or_else(|| cari(&h4, &["korwil"], &[]));
    let pagu = cari(&h4, &["pagu"], &[]);
    let mulai = cari(&h4, &["tanggal pelaksanaan"], &[]);
    let p = cari(&h4, &["pekerja"], &[]);
    let q = cari(&h4, &["hari kerja"], &[]);
    let r = cari(&h4, &["hok"], &[]);
    let tahap = h4.iter().find(|(_, v)| v.trim() == "tahap").map(|(c, _)| *c);
    let ah = cari(&h4, &["target fisik"], &[]);
    let ai = cari(&h4, &["deviasi"], &[]);
    let s = cari(&h6, &["upah"], &[]);
    let u = cari(&h6, &["material"], &[]);
    let w = cari(&h6, &["bop"], &[]);

    let lalu: Vec<u32> = h6.iter().filter(|(_, t)| t.as_str() == "minggu lalu").map(|(c, _)| *c).collect();
    let ini: Vec<u32> = h6.iter().filter(|(_, t)| t.as_str() == "minggu ini").map(|(c, _)| *c).collect();
    let kum: Vec<u32> = h6.iter().filter(|(_, t)| t.starts_with("kumulatif")).map(|(c, _)| *c).collect();
    if lalu.len() < 3 || ini.len() < 3 || kum.len() < 3 {
        return Err("header Minggu Lalu/Minggu ini/Kumulatif tidak lengkap".to_string());
    }

    let mut hilang: Vec<&str> = Vec::new();
    let mut ambil = |nm: &'static str, v: Option<u32>| -> u32 {
        v.unwrap_or_else(|| {
            hilang.push(nm);
            0
        })
    };
    let k = Kolom {
        no: 1,
        desa: ambil("desa", desa),
        nama: ambil("nama", nama),
        korfas: ambil("korfas", korfas),
        pagu: ambil("pagu", pagu),
        mulai: ambil("mulai", mulai),
        selesai: mulai.map(|m| m + 1).unwrap_or(0),
        tahap,
        p: ambil("P", p),
        q: ambil("Q", q),
        r: ambil("R", r),
        s: ambil("S", s),
        u: ambil("U", u),
        w: ambil("W", w),
        ah: ambil("AH", ah),
        y: lalu[0],
        ab: lalu[1],
        ae: lalu[2],
        z: ini[0],
        ac: ini[1],
        af: ini[2],
        aa: kum[0],
        ad: kum[1],
        ag: kum[2],
        kab: cari(&h4, &["kabupaten"], &[]),
        kec: cari(&h4, &["kecamatan"], &[]),
    };
    if ai.is_none() {
        hilang.push("AI");
    }
    if !hilang.is_empty() {
        return Err(format!("header tidak ditemukan: {}", hilang.join(", ")));
    }
    Ok(k)
}

/// Membaca semua baris Pokmas di satu sheet (semua korfas dan tahap).
fn baca_sheet(g: &Grid, nama_sheet: &str, lpk: bool, k: &Kolom) -> Vec<Row> {
    let mut rows = Vec::new();
    for r in 9..=g.max_row() {
        let gnama = g.get(r, k.nama);
        let kf = cell_text(g.get(r, k.korfas)).trim().to_string();
        let teks_nama = match gnama {
            Data::String(s) if !s.trim().is_empty() && s.trim() != "0" => Some(s.clone()),
            _ => None,
        };
        let (nama_mentah, nama_kosong) = match teks_nama {
            Some(n) => (n, false),
            None => match g.get(r, k.desa) {
                // Baris Pokmas baru yang namanya belum diisi: tetap dipakai, diberi nama sementara dari desa.
                Data::String(s) if !kf.is_empty() && !s.trim().is_empty() => {
                    (format!("(Desa {})", s.trim()), true)
                }
                _ => continue,
            },
        };

        let tahap = k.tahap.and_then(|c| {
            let d = g.get(r, c);
            if matches!(d, Data::Empty) || matches!(d, Data::String(s) if s.is_empty()) {
                None
            } else {
                Some(cell_text(d).trim().to_string())
            }
        });
        let no = match g.get(r, k.no) {
            Data::Int(i) => i.to_string(),
            Data::Float(f) => (f.round() as i64).to_string(),
            other => cell_text(other),
        };
        let num = |c: u32| as_f64(g.get(r, c)).unwrap_or(0.0);
        let terisi_angka = |c: u32| is_num(g.get(r, c));
        let mulai = tanggal(g.get(r, k.mulai), 2026);
        let selesai = tanggal(g.get(r, k.selesai), 2026);

        let wajib: Vec<String> = WAJIB
            .iter()
            .filter(|(c, _)| {
                let col = match *c {
                    "AH" => k.ah,
                    "AG" => k.ag,
                    "AD" => k.ad,
                    _ => k.aa,
                };
                !terisi_angka(col)
            })
            .map(|(_, n)| n.to_string())
            .collect();
        let mut info = Vec::new();
        if nama_kosong {
            info.push("nama Pokmas".to_string());
        }
        if !terisi_angka(k.p) {
            info.push("jumlah pekerja".to_string());
        }
        if !terisi_angka(k.af) {
            info.push("fisik minggu ini".to_string());
        }
        if mulai.is_none() || selesai.is_none() {
            info.push("tanggal pelaksanaan".to_string());
        }

        rows.push(Row {
            lpk,
            sheet: nama_sheet.to_string(),
            row: r,
            no,
            nama: tanpa_pokmas(nama_mentah.trim()),
            label: short(&nama_mentah),
            desa: cell_text(g.get(r, k.desa)),
            tahap,
            kab: k.kab.map(|c| cell_text(g.get(r, c))).filter(|s| !s.is_empty()),
            kec: k.kec.map(|c| cell_text(g.get(r, c))).filter(|s| !s.is_empty()),
            korfas: kf,
            pagu: if terisi_angka(k.pagu) { num(k.pagu) } else { 0.0 },
            mulai,
            selesai,
            p: num(k.p).round() as i64,
            q: num(k.q).round() as i64,
            r: num(k.r).round() as i64,
            s: num(k.s),
            u: num(k.u),
            w: num(k.w),
            y: num(k.y),
            z: num(k.z),
            ab: num(k.ab),
            ac: num(k.ac),
            ae: num(k.ae),
            af: num(k.af),
            aa: num(k.aa),
            ad: num(k.ad),
            ag: num(k.ag),
            ah: if terisi_angka(k.ah) { Some(num(k.ah)) } else { None },
            wajib,
            info,
            ..Default::default()
        });
    }
    rows
}

/// Membaca workbook xlsx. Sheet M<n> yang header-nya tidak lengkap dilewati.
pub fn baca_book(bytes: &[u8], lpk: bool) -> Result<Book, String> {
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|e| format!("File xlsx tidak bisa dibaca: {e}"))?;
    let re = Regex::new(r"^M(\d+)$").unwrap();
    let mut urut: Vec<(u32, String)> = wb
        .sheet_names()
        .iter()
        .filter_map(|n| re.captures(n).and_then(|c| c[1].parse().ok()).map(|i| (i, n.clone())))
        .collect();
    urut.sort();

    let mut book = Book::default();
    for (i, nama) in urut {
        book.names.push(nama.clone());
        let Ok(range) = wb.worksheet_range(&nama) else { continue };
        let g = Grid { range: &range };
        let Ok(k) = kolom(&g) else { continue };
        book.sheets.insert(i, baca_sheet(&g, &nama, lpk, &k));
    }
    if book.names.is_empty() {
        return Err("Format tidak dikenali: sheet rekap Lapming (M1, M2, ...) tidak ditemukan".to_string());
    }
    Ok(book)
}
