//! Mengisi template xlsx dengan mengganti isi sel di XML sheet. Format, rumus, dan sheet lain tetap utuh.
//! Sel yang ditulis harus sudah ada di template (sel kosong ber-style); kalau tidak, penulisan ditolak.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use regex::Regex;

pub enum Nilai {
    Teks(String),
    Angka(f64),
}

/// (nama sheet, referensi sel seperti "C5", nilai)
pub type Isian = (String, String, Nilai);

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Path XML sheet di dalam zip, dicocokkan dari nama sheet di workbook.xml dan relasinya.
fn peta_sheet(wb: &str, rels: &str) -> Result<HashMap<String, String>, String> {
    let re_sheet = Regex::new(r#"<sheet [^>]*?name="([^"]*)"[^>]*?r:id="([^"]*)""#).unwrap();
    let re_rel = Regex::new(r#"<Relationship [^>]*?Id="([^"]*)"[^>]*?Target="([^"]*)""#).unwrap();
    let mut target: HashMap<&str, &str> = HashMap::new();
    for c in re_rel.captures_iter(rels) {
        target.insert(c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str());
    }
    let mut out = HashMap::new();
    for c in re_sheet.captures_iter(wb) {
        let nama = xml_unescape(c.get(1).unwrap().as_str());
        let rid = c.get(2).unwrap().as_str();
        let t = target.get(rid).ok_or_else(|| format!("Relasi {rid} tidak ditemukan"))?;
        let t = t.trim_start_matches('/');
        let path = if t.starts_with("xl/") { t.to_string() } else { format!("xl/{t}") };
        out.insert(nama, path);
    }
    Ok(out)
}

fn sel_baru(r: &str, s_attr: &str, nilai: &Nilai) -> String {
    match nilai {
        Nilai::Teks(t) => format!(
            r#"<c r="{r}"{s_attr} t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
            xml_escape(t)
        ),
        Nilai::Angka(a) => {
            let a = if a.is_finite() { *a } else { 0.0 };
            format!(r#"<c r="{r}"{s_attr}><v>{a}</v></c>"#)
        }
    }
}

/// Mengganti sel-sel tertentu di satu XML sheet. Mengembalikan error kalau ada sel yang tidak ditemukan.
fn isi_sheet(xml: &str, isian: &HashMap<String, &Nilai>) -> Result<String, String> {
    let re_s = Regex::new(r#" s="(\d+)""#).unwrap();
    let re_sel = Regex::new(r#"<c r="([A-Z]+[0-9]+)"([^>]*?)(?:/>|>.*?</c>)"#).unwrap();
    let mut ditemukan: HashMap<String, bool> = isian.keys().map(|k| (k.clone(), false)).collect();
    let hasil = re_sel.replace_all(xml, |c: &regex::Captures| {
        let r = c.get(1).unwrap().as_str();
        match isian.get(r) {
            Some(nilai) => {
                ditemukan.insert(r.to_string(), true);
                let s_attr = re_s
                    .captures(c.get(2).unwrap().as_str())
                    .map(|m| format!(r#" s="{}""#, &m[1]))
                    .unwrap_or_default();
                sel_baru(r, &s_attr, nilai)
            }
            None => c.get(0).unwrap().as_str().to_string(),
        }
    });
    let hilang: Vec<&String> = ditemukan.iter().filter(|(_, v)| !**v).map(|(k, _)| k).collect();
    if !hilang.is_empty() {
        let mut daftar: Vec<&str> = hilang.iter().map(|s| s.as_str()).collect();
        daftar.sort();
        return Err(format!("Sel tidak ada di template: {}", daftar.join(", ")));
    }
    Ok(hasil.into_owned())
}

fn baca(berkas: &[(String, Vec<u8>)], nama: &str) -> Result<String, String> {
    let (_, isi) = berkas.iter().find(|(n, _)| n == nama).ok_or_else(|| format!("{nama} tidak ada di template"))?;
    String::from_utf8(isi.clone()).map_err(|e| e.to_string())
}

/// Membaca template, mengisi semua isian, dan mengembalikan xlsx baru.
pub fn isi_template(template: &[u8], isian: &[Isian]) -> Result<Vec<u8>, String> {
    let mut zin = zip::ZipArchive::new(Cursor::new(template)).map_err(|e| e.to_string())?;
    let mut berkas: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).map_err(|e| e.to_string())?;
        let mut isi = Vec::new();
        f.read_to_end(&mut isi).map_err(|e| e.to_string())?;
        berkas.push((f.name().to_string(), isi));
    }
    let peta = peta_sheet(&baca(&berkas, "xl/workbook.xml")?, &baca(&berkas, "xl/_rels/workbook.xml.rels")?)?;

    // Kelompokkan isian per file sheet.
    let mut per_file: HashMap<String, HashMap<String, &Nilai>> = HashMap::new();
    for (sheet, sel, nilai) in isian {
        let path = peta.get(sheet).ok_or_else(|| format!("Sheet '{sheet}' tidak ada di template"))?;
        per_file.entry(path.clone()).or_default().insert(sel.clone(), nilai);
    }
    for (path, sel) in &per_file {
        let xml = baca(&berkas, path)?;
        let xml_baru = isi_sheet(&xml, sel)?;
        if let Some((_, isi)) = berkas.iter_mut().find(|(n, _)| n == path) {
            *isi = xml_baru.into_bytes();
        }
    }

    let mut out = Cursor::new(Vec::new());
    {
        let mut zout = zip::ZipWriter::new(&mut out);
        let opsi = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (nama, isi) in &berkas {
            zout.start_file(nama.as_str(), opsi).map_err(|e| e.to_string())?;
            zout.write_all(isi).map_err(|e| e.to_string())?;
        }
        zout.finish().map_err(|e| e.to_string())?;
    }
    Ok(out.into_inner())
}
