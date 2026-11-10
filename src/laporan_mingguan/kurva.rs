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
    /// Selisih per minggu; None untuk minggu yang belum ada datanya.
    pub rencana: Vec<Option<f64>>,
    pub realisasi: Vec<Option<f64>>,
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

const BULAN: [&str; 12] = [
    "JANUARI", "FEBRUARI", "MARET", "APRIL", "MEI", "JUNI", "JULI", "AGUSTUS", "SEPTEMBER", "OKTOBER", "NOVEMBER", "DESEMBER",
];
const ROMAWI: [&str; 6] = ["I", "II", "III", "IV", "V", "VI"];

/// Periode (mulai, selesai) minggu Lapming ke-`w`. Diambil dari tanggal yang paling banyak dipakai di sheet
/// minggu itu; minggu tanpa tanggal dihitung dari minggu terdekat yang punya tanggal, atau dari 1 September 2026.
pub fn periode_minggu(riwayat: &[(u32, Vec<Row>)], w: u32) -> (NaiveDate, NaiveDate) {
    let mut dikenal: Vec<(u32, NaiveDate, NaiveDate)> = Vec::new();
    for (no, rows) in riwayat {
        let mut hitung: Vec<((NaiveDate, NaiveDate), usize)> = Vec::new();
        for x in rows {
            if let (Some(a), Some(b)) = (x.mulai, x.selesai) {
                if b < a {
                    continue;
                }
                match hitung.iter_mut().find(|(k, _)| *k == (a, b)) {
                    Some((_, n)) => *n += 1,
                    None => hitung.push(((a, b), 1)),
                }
            }
        }
        if let Some(((a, b), _)) = hitung.into_iter().max_by_key(|(_, n)| *n) {
            dikenal.push((*no, a, b));
        }
    }
    if let Some((_, a, b)) = dikenal.iter().find(|(no, _, _)| *no == w) {
        return (*a, *b);
    }
    match dikenal.iter().min_by_key(|(no, _, _)| (*no as i64 - w as i64).abs()) {
        Some((no, a, b)) => {
            let geser = chrono::Duration::days(7 * (w as i64 - *no as i64));
            (*a + geser, *b + geser)
        }
        None => {
            let a = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap() + chrono::Duration::days(7 * (w as i64 - 1));
            (a, a + chrono::Duration::days(6))
        }
    }
}

/// Blok KURVA-S dari data Lapming: satu blok per lokasi, satu kolom per minggu Lapming (M1 di kolom pertama).
pub fn dari_riwayat(rows: &[Row], riwayat: &[(u32, Vec<Row>)]) -> Vec<Blok> {
    let terakhir = riwayat.iter().map(|(w, _)| *w).max().unwrap_or(0);
    rows.iter()
        .map(|x| {
            let mut rencana = Vec::with_capacity(PERIODE);
            let mut realisasi = Vec::with_capacity(PERIODE);
            let (mut kum_r, mut kum_s) = (0.0, 0.0);
            for w in 1..=PERIODE as u32 {
                if w > terakhir {
                    rencana.push(None);
                    realisasi.push(None);
                    continue;
                }
                // Minggu tanpa data (atau target kosong) memakai kumulatif minggu sebelumnya.
                let baris = riwayat.iter().find(|(no, _)| *no == w).and_then(|(_, rs)| rs.iter().find(|y| y.lpk == x.lpk && y.no == x.no));
                let (r, s) = match baris {
                    Some(y) => (y.ah.unwrap_or(kum_r), y.ag),
                    None => (kum_r, kum_s),
                };
                rencana.push(Some(r - kum_r));
                realisasi.push(Some(s - kum_s));
                kum_r = r;
                kum_s = s;
            }
            Blok {
                kab: x.kab.clone().unwrap_or_default(),
                kec: x.kec.clone().unwrap_or_default(),
                desa: x.desa.clone(),
                nama: x.nama.clone(),
                rencana,
                realisasi,
            }
        })
        .collect()
}

/// Header periode (baris 9-11, kolom I..AQ): bulan, minggu ke- dalam bulan itu, dan rentang tanggal.
/// Mengembalikan XML baris yang sudah diganti dan daftar merge bulan.
fn tulis_header(mut atas: String, kalender: &[(NaiveDate, NaiveDate)]) -> (String, Vec<String>) {
    use chrono::Datelike;
    let mut merge = Vec::new();
    let ganti = |xml: &mut String, sel: &str, isi: Option<String>| {
        let re = Regex::new(&format!(r#"(?s)<c r="{sel}"([^>]*?)(?:/>|>.*?</c>)"#)).unwrap();
        let re_s = Regex::new(r#" s="\d+""#).unwrap();
        if let Some(m) = re.captures(xml) {
            let s = re_s.find(&m[1]).map(|v| v.as_str().to_string()).unwrap_or_default();
            let baru = match isi {
                Some(t) => format!(r#"<c r="{sel}"{s} t="inlineStr"><is><t>{}</t></is></c>"#, xml_escape(&t)),
                None => format!(r#"<c r="{sel}"{s}/>"#),
            };
            let rentang = m.get(0).unwrap().range();
            xml.replace_range(rentang, &baru);
        }
    };
    let mut awal_bulan = 0usize;
    let mut ke = 0usize;
    for (p, (mulai, selesai)) in kalender.iter().enumerate() {
        let kol = huruf_kolom(KOL_PERIODE_AWAL + p as u32);
        let bulan_baru = p == 0 || (kalender[p - 1].0.month(), kalender[p - 1].0.year()) != (mulai.month(), mulai.year());
        if bulan_baru {
            if p > 0 && p - awal_bulan > 1 {
                merge.push(format!(
                    r#"<mergeCell ref="{}9:{}9"/>"#,
                    huruf_kolom(KOL_PERIODE_AWAL + awal_bulan as u32),
                    huruf_kolom(KOL_PERIODE_AWAL + p as u32 - 1)
                ));
            }
            awal_bulan = p;
            ke = 0;
        }
        ganti(&mut atas, &format!("{kol}9"), bulan_baru.then(|| format!("{} {}", BULAN[mulai.month0() as usize], mulai.year())));
        ganti(&mut atas, &format!("{kol}10"), Some(ROMAWI[ke.min(ROMAWI.len() - 1)].to_string()));
        ganti(&mut atas, &format!("{kol}11"), Some(format!("{} s/d {}", mulai.day(), selesai.day())));
        ke += 1;
    }
    if kalender.len() - awal_bulan > 1 {
        merge.push(format!(
            r#"<mergeCell ref="{}9:{}9"/>"#,
            huruf_kolom(KOL_PERIODE_AWAL + awal_bulan as u32),
            huruf_kolom(KOL_PERIODE_AWAL + kalender.len() as u32 - 1)
        ));
    }
    (atas, merge)
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
pub fn tulis(template: &[u8], blok: &[Blok], kalender: &[(NaiveDate, NaiveDate)]) -> Result<Vec<u8>, String> {
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
    // Minggu yang belum punya data di lokasi mana pun: kumulatif, deviasi, dan rata-rata dibiarkan kosong.
    let ada_data: Vec<bool> = (0..PERIODE)
        .map(|p| blok.iter().any(|b| b.rencana.get(p).copied().flatten().is_some()))
        .collect();
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
                if !ada_data[p] && off != 0 && off != 2 {
                    kolom.push((kol.clone(), Isi::Kosong));
                    continue;
                }
                let isi = match off {
                    0 => b.rencana.get(p).copied().flatten().map(Isi::Angka).unwrap_or(Isi::Kosong),
                    2 => b.realisasi.get(p).copied().flatten().map(Isi::Angka).unwrap_or(Isi::Kosong),
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
            for (p, kol) in kol_periode.iter().enumerate() {
                if !ada_data[p] {
                    sel_baris.push(sel(&gaya, off, kol, baris, &Isi::Kosong));
                    continue;
                }
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
    let (atas, merge_bulan) = tulis_header(atas, kalender);
    // Merge bulan lama di baris 9 (kolom periode) diganti dengan yang baru.
    let kol_periode_set: std::collections::HashSet<&str> = kol_periode.iter().map(|k| k.as_str()).collect();
    let merge_lama: Vec<String> = re_merge
        .captures_iter(ekor)
        .filter(|c| c[2].parse::<u32>().unwrap() < BARIS_AWAL)
        .filter(|c| !(&c[2] == "9" && kol_periode_set.contains(&c[1])))
        .map(|c| c[0].to_string())
        .collect();
    let semua_merge: Vec<String> = merge_lama.into_iter().chain(merge_bulan).chain(merge).collect();
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
                rencana: vec![Some(0.01); PERIODE],
                realisasi: vec![Some(0.005); PERIODE],
            })
            .collect();
        let kalender: Vec<(NaiveDate, NaiveDate)> = (1..=PERIODE as u32).map(|w| periode_minggu(&[], w)).collect();
        let out = tulis(template, &blok, &kalender).unwrap();
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
        // Header dibuat dari kalender minggu: tanpa data, minggu 1 mulai 1 September 2026.
        assert_eq!(teks(9, 9), "SEPTEMBER 2026");
        assert_eq!(teks(10, 9), "I");
        assert_eq!(teks(11, 9), "1 s/d 7");
        assert_eq!(teks(9, 14), "OKTOBER 2026");
    }
}
