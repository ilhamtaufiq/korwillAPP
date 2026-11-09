//! KURVA-S dengan jumlah blok sesuai jumlah lokasi. Baris 13 ke bawah ditulis ulang dari sel prototipe
//! di template (gaya dan merge per blok), sehingga judul, header, dan rumus di atas baris 13 tetap utuh.
//!
//! Tiap blok 5 baris: rencana (selisih per periode), rencana kumulatif, realisasi (selisih),
//! realisasi kumulatif, dan deviasi. Ringkasan di bawah memakai rata-rata antar lokasi.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use chrono::NaiveDate;
use regex::Regex;

use super::isi::xml_escape;
use crate::lapming::model::Row;

const BARIS_AWAL: u32 = 13;
const TINGGI_BLOK: u32 = 5;
/// Kolom periode I..=AQ (35 periode mingguan).
pub const PERIODE: usize = 35;
const KOL_PERIODE_AWAL: u32 = 9;
/// Kolom tetap di setiap blok yang digabung vertikal (A..G dan AR..AS).
const KOLOM_MERGE: [&str; 9] = ["A", "B", "C", "D", "E", "F", "G", "AR", "AS"];

/// Isi satu lokasi: identitas dan selisih per periode.
#[derive(Clone, Debug, Default)]
pub struct Blok {
    pub kab: String,
    pub kec: String,
    pub desa: String,
    pub nama: String,
    pub rencana: Vec<f64>,
    pub realisasi: Vec<f64>,
}

pub fn huruf_kolom(mut c: u32) -> String {
    let mut out = String::new();
    while c > 0 {
        let sisa = (c - 1) % 26;
        out.insert(0, (b'A' + sisa as u8) as char);
        c = (c - 1) / 26;
    }
    out
}

fn indeks_kolom(huruf: &str) -> u32 {
    huruf.bytes().fold(0, |n, b| n * 26 + (b - b'A' + 1) as u32)
}

/// Akhir periode KURVA-S ke-p (0-based): 19 Maret 2026, lalu tiap 7 hari.
pub fn akhir_periode(p: usize) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 3, 19).unwrap() + chrono::Duration::days(7 * p as i64)
}

/// Akhir minggu Lapming ke-w: minggu 1 berakhir 7 September 2026.
pub fn akhir_minggu(w: u32) -> NaiveDate {
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

/// Blok KURVA-S dari data Lapming: satu blok per lokasi, urut seperti daftar baris.
pub fn dari_riwayat(rows: &[Row], riwayat: &[(u32, Vec<Row>)]) -> Vec<Blok> {
    rows.iter()
        .map(|x| {
            let mut rencana_kum = Vec::with_capacity(PERIODE);
            let mut realisasi_kum = Vec::with_capacity(PERIODE);
            for p in 0..PERIODE {
                let (r, s) = kumulatif_pada(riwayat, x.lpk, &x.no, p);
                rencana_kum.push(r);
                realisasi_kum.push(s);
            }
            let selisih = |kum: &[f64]| -> Vec<f64> {
                kum.iter().enumerate().map(|(p, v)| if p == 0 { *v } else { v - kum[p - 1] }).collect()
            };
            Blok {
                kab: x.kab.clone().unwrap_or_default(),
                kec: x.kec.clone().unwrap_or_default(),
                desa: x.desa.clone(),
                nama: x.nama.clone(),
                rencana: selisih(&rencana_kum),
                realisasi: selisih(&realisasi_kum),
            }
        })
        .collect()
}

/// Gaya tiap sel prototipe (baris 13..17 template), per offset baris dan huruf kolom.
fn prototipe_gaya(xml: &str) -> HashMap<(u32, String), String> {
    let re_baris = Regex::new(r#"(?s)<row r="(\d+)"[^>]*?(?:/>|>(.*?)</row>)"#).unwrap();
    let re_sel = Regex::new(r#"<c r="([A-Z]+)(\d+)"([^>]*?)(?:/>|>.*?</c>)"#).unwrap();
    let re_s = Regex::new(r#" s="(\d+)""#).unwrap();
    let mut out = HashMap::new();
    for b in re_baris.captures_iter(xml) {
        let r: u32 = b[1].parse().unwrap();
        if !(BARIS_AWAL..BARIS_AWAL + TINGGI_BLOK).contains(&r) {
            continue;
        }
        let isi = b.get(2).map(|m| m.as_str()).unwrap_or("");
        for s in re_sel.captures_iter(isi) {
            if let Some(m) = re_s.captures(&s[3]) {
                out.insert(((r - BARIS_AWAL), s[1].to_string()), m[1].to_string());
            }
        }
    }
    out
}

enum Isi {
    Kosong,
    Teks(String),
    Angka(f64),
    Rumus(String),
}

fn sel(gaya: &HashMap<(u32, String), String>, offset: u32, kol: &str, baris: u32, isi: &Isi) -> String {
    let s = gaya.get(&(offset, kol.to_string())).map(|v| format!(r#" s="{v}""#)).unwrap_or_default();
    match isi {
        Isi::Kosong => format!(r#"<c r="{kol}{baris}"{s}/>"#),
        Isi::Teks(t) => format!(
            r#"<c r="{kol}{baris}"{s} t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
            xml_escape(t)
        ),
        Isi::Angka(a) => format!(r#"<c r="{kol}{baris}"{s}><v>{}</v></c>"#, if a.is_finite() { *a } else { 0.0 }),
        Isi::Rumus(f) => format!(r#"<c r="{kol}{baris}"{s}><f>{}</f></c>"#, xml_escape(f)),
    }
}

/// Menulis ulang KURVA-S di template dengan satu blok per lokasi dan ringkasan rata-rata.
pub fn tulis(template: &[u8], blok: &[Blok]) -> Result<Vec<u8>, String> {
    let mut zin = zip::ZipArchive::new(Cursor::new(template)).map_err(|e| e.to_string())?;
    let mut berkas: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).map_err(|e| e.to_string())?;
        let mut isi = Vec::new();
        f.read_to_end(&mut isi).map_err(|e| e.to_string())?;
        berkas.push((f.name().to_string(), isi));
    }
    let path = "xl/worksheets/sheet15.xml";
    let xml = {
        let (_, isi) = berkas.iter().find(|(n, _)| n == path).ok_or("sheet KURVA-S tidak ada di template")?;
        String::from_utf8(isi.clone()).map_err(|e| e.to_string())?
    };
    let gaya = prototipe_gaya(&xml);
    let attr_baris = Regex::new(r#"<row r="13"([^>]*)>"#)
        .unwrap()
        .captures(&xml)
        .map(|c| c[1].to_string())
        .unwrap_or_default();

    // Baris dari blok dan ringkasan.
    let mut baris_xml: Vec<String> = Vec::new();
    let mut merge: Vec<String> = Vec::new();
    let jumlah = blok.len() as u32;
    let kol_periode: Vec<String> = (0..PERIODE as u32).map(|p| huruf_kolom(KOL_PERIODE_AWAL + p)).collect();
    for (k, b) in blok.iter().enumerate() {
        let base = BARIS_AWAL + TINGGI_BLOK * k as u32;
        for off in 0..TINGGI_BLOK {
            let baris = base + off;
            let mut sel_baris: Vec<String> = Vec::new();
            let mut kolom: Vec<(String, Isi)> = vec![
                ("A".into(), if off == 0 { Isi::Angka((k + 1) as f64) } else { Isi::Kosong }),
                ("B".into(), if off == 0 { Isi::Teks(b.kab.clone()) } else { Isi::Kosong }),
                ("C".into(), if off == 0 { Isi::Teks(b.kec.clone()) } else { Isi::Kosong }),
                ("D".into(), if off == 0 { Isi::Teks(b.desa.clone()) } else { Isi::Kosong }),
                ("E".into(), if off == 0 { Isi::Teks(b.nama.clone()) } else { Isi::Kosong }),
                ("F".into(), Isi::Kosong),
                ("G".into(), Isi::Kosong),
                ("H".into(), match off { 0 => Isi::Teks("RENCANA".into()), 2 => Isi::Teks("REALISASI".into()), 4 => Isi::Teks("DEVIASI".into()), _ => Isi::Kosong }),
            ];
            for (p, kol) in kol_periode.iter().enumerate() {
                let isi = match off {
                    0 => Isi::Angka(*b.rencana.get(p).unwrap_or(&0.0)),
                    2 => Isi::Angka(*b.realisasi.get(p).unwrap_or(&0.0)),
                    1 => Isi::Rumus(if p == 0 { format!("{kol}{}", base) } else { format!("{}{}+{kol}{}", kol_periode[p - 1], base + 1, base) }),
                    3 => Isi::Rumus(if p == 0 { format!("{kol}{}", base + 2) } else { format!("{}{}+{kol}{}", kol_periode[p - 1], base + 3, base + 2) }),
                    _ => Isi::Rumus(format!("{kol}{}-{kol}{}", base + 3, base + 1)),
                };
                kolom.push((kol.clone(), isi));
            }
            kolom.push(("AR".into(), Isi::Kosong));
            kolom.push(("AS".into(), Isi::Kosong));
            for (kol, isi) in &kolom {
                sel_baris.push(sel(&gaya, off, kol, baris, isi));
            }
            baris_xml.push(format!(r#"<row r="{baris}"{attr_baris}>{}</row>"#, sel_baris.join("")));
        }
        for kol in KOLOM_MERGE {
            merge.push(format!(r#"<mergeCell ref="{kol}{base}:{kol}{}"/>"#, base + TINGGI_BLOK - 1));
        }
    }

    // Ringkasan rata-rata antar lokasi, di bawah semua blok.
    if jumlah > 0 {
        let base = BARIS_AWAL + TINGGI_BLOK * jumlah;
        let blok_ke = |off: u32| -> Vec<u32> { (0..jumlah).map(|k| BARIS_AWAL + TINGGI_BLOK * k + off).collect() };
        let rata = |kol: &str, off: u32| -> String {
            let daftar: Vec<String> = blok_ke(off).iter().map(|r| format!("{kol}{r}")).collect();
            format!("AVERAGE({})", daftar.join(","))
        };
        for off in 0..TINGGI_BLOK {
            let baris = base + off;
            let mut sel_baris = Vec::new();
            let label = match off { 0 => Some("RENCANA"), 2 => Some("REALISASI"), 4 => Some("DEVIASI"), _ => None };
            sel_baris.push(sel(&gaya, off, "A", baris, &if off == 0 { Isi::Teks("Rata-rata antar lokasi".into()) } else { Isi::Kosong }));
            for kol in ["B", "C", "D", "E", "F", "G"] {
                sel_baris.push(sel(&gaya, off, kol, baris, &Isi::Kosong));
            }
            sel_baris.push(sel(&gaya, off, "H", baris, &label.map(|t| Isi::Teks(t.into())).unwrap_or(Isi::Kosong)));
            for kol in &kol_periode {
                let isi = match off {
                    0 => Isi::Rumus(rata(kol, 0)),
                    1 => Isi::Rumus(rata(kol, 1)),
                    2 => Isi::Rumus(rata(kol, 2)),
                    3 => Isi::Rumus(rata(kol, 3)),
                    _ => Isi::Rumus(format!("{kol}{}-{kol}{}", base + 3, base + 1)),
                };
                sel_baris.push(sel(&gaya, off, kol, baris, &isi));
            }
            sel_baris.push(sel(&gaya, off, "AR", baris, &Isi::Kosong));
            sel_baris.push(sel(&gaya, off, "AS", baris, &Isi::Kosong));
            baris_xml.push(format!(r#"<row r="{baris}"{attr_baris}>{}</row>"#, sel_baris.join("")));
        }
    }

    // Susun ulang sheet: baris di atas 13 dipertahankan, merge baris 13+ diganti.
    let re_baris_lama = Regex::new(r#"(?s)<row r="(\d+)"[^>]*?(?:/>|>.*?</row>)"#).unwrap();
    let mut atas = String::new();
    for m in re_baris_lama.captures_iter(&xml) {
        if m[1].parse::<u32>().unwrap() < BARIS_AWAL {
            atas.push_str(m.get(0).unwrap().as_str());
        }
    }
    let awal = xml.find("<sheetData>").ok_or("sheetData tidak ada")? + "<sheetData>".len();
    let akhir = xml.find("</sheetData>").ok_or("sheetData tidak ada")?;
    let kepala = &xml[..awal];
    let ekor = &xml[akhir..];
    let re_merge = Regex::new(r#"<mergeCell ref="([A-Z]+)(\d+)(?::([A-Z]+)(\d+))?"/>"#).unwrap();
    let merge_lama: Vec<String> = re_merge
        .captures_iter(ekor)
        .filter(|c| c[2].parse::<u32>().unwrap() < BARIS_AWAL)
        .map(|c| c[0].to_string())
        .collect();
    let semua_merge: Vec<String> = merge_lama.into_iter().chain(merge).collect();
    let re_merge_blok = Regex::new(r#"<mergeCells count="\d+">.*?</mergeCells>"#).unwrap();
    let ekor_baru = re_merge_blok
        .replace(ekor, format!(r#"<mergeCells count="{}">{}</mergeCells>"#, semua_merge.len(), semua_merge.join("")).as_str())
        .into_owned();
    let xml_baru = format!("{kepala}{atas}{}{ekor_baru}", baris_xml.join(""));

    let mut out = Cursor::new(Vec::new());
    {
        let mut zout = zip::ZipWriter::new(&mut out);
        let opsi = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (nama, isi) in &berkas {
            zout.start_file(nama.as_str(), opsi).map_err(|e| e.to_string())?;
            let bytes: &[u8] = if nama == path { xml_baru.as_bytes() } else { isi };
            zout.write_all(bytes).map_err(|e| e.to_string())?;
        }
        zout.finish().map_err(|e| e.to_string())?;
    }
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huruf_kolom_dan_indeksnya_cocok() {
        assert_eq!(huruf_kolom(9), "I");
        assert_eq!(huruf_kolom(43), "AQ");
        assert_eq!(indeks_kolom("AQ"), 43);
    }

    #[test]
    fn jumlah_blok_mengikuti_jumlah_lokasi() {
        let template = include_bytes!("../../templates/laporan_mingguan.xlsx");
        let blok: Vec<Blok> = (0..16)
            .map(|k| Blok {
                kab: "Bogor".into(),
                kec: "Ciomas".into(),
                desa: format!("Desa {k}"),
                nama: format!("Pokmas {k}"),
                rencana: vec![0.01; PERIODE],
                realisasi: vec![0.005; PERIODE],
            })
            .collect();
        let out = tulis(template, &blok).unwrap();
        let mut wb: calamine::Xlsx<_> = calamine::open_workbook_from_rs(std::io::Cursor::new(out)).unwrap();
        let r = calamine::Reader::worksheet_range(&mut wb, "KURVA-S").unwrap();
        let (r0, c0) = r.start().unwrap_or((0, 0));
        let teks = |row: u32, col: u32| -> String {
            match r.get(((row - 1 - r0) as usize, (col - 1 - c0) as usize)) {
                Some(calamine::Data::String(s)) => s.clone(),
                _ => String::new(),
            }
        };
        // Blok terakhir (lokasi ke-16) mulai di 13 + 5*15 = 88; ringkasan di 13 + 5*16 = 93.
        assert_eq!(teks(88, 4), "Desa 15");
        assert_eq!(teks(93, 1), "Rata-rata antar lokasi");
        assert_eq!(teks(93, 8), "RENCANA");
        assert_eq!(teks(97, 8), "DEVIASI");
    }
}
