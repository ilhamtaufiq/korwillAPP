//! Mengisi template xlsx dengan mengganti isi sel di XML sheet. Format, rumus, dan sheet lain tetap utuh.
//! Sel yang ditulis harus sudah ada di template (sel kosong ber-style); kalau tidak, penulisan ditolak.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};

use regex::Regex;

pub enum Nilai {
    Teks(String),
    Angka(f64),
    /// Nomor seri tanggal Excel (hari sejak 1899-12-30). Kalau style sel di template tidak berformat
    /// tanggal, sel diberi salinan style yang sama dengan format `dd-mm-yyyy` supaya tampil sebagai tanggal.
    Tanggal(f64),
    /// Rumus Excel tanpa tanda "=" di depan. Nilainya dihitung Excel saat file dibuka.
    Rumus(String),
}

/// (nama sheet, referensi sel seperti "C5", nilai)
pub type Isian = (String, String, Nilai);

pub(super) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub(super) fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Path XML sheet di dalam zip, dicocokkan dari nama sheet di workbook.xml dan relasinya.
pub(super) fn peta_sheet(wb: &str, rels: &str) -> Result<HashMap<String, String>, String> {
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
        Nilai::Angka(a) | Nilai::Tanggal(a) => {
            let a = if a.is_finite() { *a } else { 0.0 };
            format!(r#"<c r="{r}"{s_attr}><v>{a}</v></c>"#)
        }
        Nilai::Rumus(f) => format!(r#"<c r="{r}"{s_attr}><f>{}</f></c>"#, xml_escape(f)),
    }
}

/// Format angka bawaan Excel yang berupa tanggal/waktu.
fn bawaan_tanggal(id: u32) -> bool {
    matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58)
}

/// Kode format kustom berupa tanggal kalau, setelah teks kutip, `[...]`, dan karakter escape dibuang,
/// masih ada kode d/m/y.
fn kode_tanggal(kode: &str) -> bool {
    let kode = xml_unescape(kode);
    let mut sisa = String::new();
    let mut it = kode.chars();
    while let Some(ch) = it.next() {
        match ch {
            '"' => {
                for c in it.by_ref() {
                    if c == '"' {
                        break;
                    }
                }
            }
            '[' => {
                for c in it.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                it.next();
            }
            c => sisa.push(c.to_ascii_lowercase()),
        }
    }
    sisa.contains('d') || sisa.contains('m') || sisa.contains('y')
}

/// Kode format yang dipakai untuk sel tanggal yang di template belum berformat tanggal.
const KODE_TANGGAL: &str = r"dd\-mm\-yyyy";

/// Pemetaan style sel (indeks cellXfs) ke style yang berformat tanggal, dengan salinan xf baru seperlunya.
struct GayaTanggal {
    xfs: Vec<String>,
    tanggal: Vec<bool>,
    fmt_id: u32,
    numfmt_baru: Option<String>,
    baru: Vec<String>,
    peta: HashMap<u32, u32>,
}

impl GayaTanggal {
    fn baca(styles: &str) -> Result<Self, String> {
        let re_numfmt = Regex::new(r#"<numFmt numFmtId="(\d+)" formatCode="([^"]*)"\s*/>"#).unwrap();
        let mut kustom: HashMap<u32, String> = HashMap::new();
        for c in re_numfmt.captures_iter(styles) {
            kustom.insert(c[1].parse().unwrap_or(0), c[2].to_string());
        }
        let blok = Regex::new(r"(?s)<cellXfs[^>]*>(.*?)</cellXfs>")
            .unwrap()
            .captures(styles)
            .ok_or("cellXfs tidak ada di styles.xml")?
            .get(1)
            .unwrap()
            .as_str()
            .to_string();
        let re_xf = Regex::new(r"(?s)<xf\b[^>]*?/>|<xf\b[^>]*>.*?</xf>").unwrap();
        let re_id = Regex::new(r#"numFmtId="(\d+)""#).unwrap();
        let xfs: Vec<String> = re_xf.find_iter(&blok).map(|m| m.as_str().to_string()).collect();
        let tanggal = xfs
            .iter()
            .map(|xf| {
                let id: u32 = re_id.captures(xf).and_then(|c| c[1].parse().ok()).unwrap_or(0);
                bawaan_tanggal(id) || kustom.get(&id).is_some_and(|k| kode_tanggal(k))
            })
            .collect();
        let ada = kustom.iter().find(|(_, k)| xml_unescape(k) == KODE_TANGGAL).map(|(id, _)| *id);
        let (fmt_id, numfmt_baru) = match ada {
            Some(id) => (id, None),
            None => {
                let id = kustom.keys().copied().max().unwrap_or(163).max(163) + 1;
                (id, Some(format!(r#"<numFmt numFmtId="{id}" formatCode="{}"/>"#, xml_escape(KODE_TANGGAL))))
            }
        };
        Ok(Self { xfs, tanggal, fmt_id, numfmt_baru, baru: Vec::new(), peta: HashMap::new() })
    }

    /// Style untuk sel tanggal: style lama kalau sudah tanggal, selain itu salinannya dengan format tanggal.
    fn untuk(&mut self, s: u32) -> u32 {
        if self.tanggal.get(s as usize).copied().unwrap_or(false) {
            return s;
        }
        if let Some(&t) = self.peta.get(&s) {
            return t;
        }
        let asal = self.xfs.get(s as usize).cloned().unwrap_or_else(|| r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#.into());
        let mut xf = Regex::new(r#"numFmtId="\d+""#)
            .unwrap()
            .replace(&asal, format!(r#"numFmtId="{}""#, self.fmt_id).as_str())
            .into_owned();
        if !xf.contains("applyNumberFormat") {
            xf = xf.replacen("<xf ", r#"<xf applyNumberFormat="1" "#, 1);
        } else {
            xf = xf.replace(r#"applyNumberFormat="0""#, r#"applyNumberFormat="1""#);
        }
        let t = (self.xfs.len() + self.baru.len()) as u32;
        self.baru.push(xf);
        self.peta.insert(s, t);
        t
    }

    /// styles.xml baru dengan xf (dan numFmt) tambahan. Tidak berubah kalau tidak ada yang ditambahkan.
    fn tulis(&self, styles: &str) -> String {
        if self.baru.is_empty() {
            return styles.to_string();
        }
        let mut out = styles.to_string();
        if let Some(nf) = &self.numfmt_baru {
            let re_count = Regex::new(r#"<numFmts count="(\d+)">"#).unwrap();
            out = if let Some(c) = re_count.captures(&out) {
                let n: u32 = c[1].parse().unwrap_or(0);
                let lama = c[0].to_string();
                out.replacen(&lama, &format!(r#"<numFmts count="{}">"#, n + 1), 1).replacen("</numFmts>", &format!("{nf}</numFmts>"), 1)
            } else {
                Regex::new(r"<styleSheet\b[^>]*>")
                    .unwrap()
                    .replace(&out, |c: &regex::Captures| format!(r#"{}<numFmts count="1">{nf}</numFmts>"#, &c[0]))
                    .into_owned()
            };
        }
        let total = self.xfs.len() + self.baru.len();
        out = Regex::new(r#"<cellXfs count="\d+">"#)
            .unwrap()
            .replace(&out, format!(r#"<cellXfs count="{total}">"#).as_str())
            .into_owned();
        out.replacen("</cellXfs>", &format!("{}</cellXfs>", self.baru.concat()), 1)
    }
}

/// Mengganti sel-sel tertentu di satu XML sheet. Mengembalikan error kalau ada sel yang tidak ditemukan.
fn isi_sheet(xml: &str, isian: &HashMap<String, &Nilai>, gaya: &mut GayaTanggal) -> Result<String, String> {
    let re_s = Regex::new(r#" s="(\d+)""#).unwrap();
    let re_sel = Regex::new(r#"<c r="([A-Z]+[0-9]+)"([^>]*?)(?:/>|>.*?</c>)"#).unwrap();
    let mut ditemukan: HashMap<String, bool> = isian.keys().map(|k| (k.clone(), false)).collect();
    let hasil = re_sel.replace_all(xml, |c: &regex::Captures| {
        let r = c.get(1).unwrap().as_str();
        match isian.get(r) {
            Some(nilai) => {
                ditemukan.insert(r.to_string(), true);
                let s: Option<u32> = re_s.captures(c.get(2).unwrap().as_str()).and_then(|m| m[1].parse().ok());
                let s = match nilai {
                    Nilai::Tanggal(_) => Some(gaya.untuk(s.unwrap_or(0))),
                    _ => s,
                };
                let s_attr = s.map(|s| format!(r#" s="{s}""#)).unwrap_or_default();
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
    let styles = baca(&berkas, "xl/styles.xml")?;
    let mut gaya = GayaTanggal::baca(&styles)?;
    for (path, sel) in &per_file {
        let xml = baca(&berkas, path)?;
        let xml_baru = isi_sheet(&xml, sel, &mut gaya)?;
        if let Some((_, isi)) = berkas.iter_mut().find(|(n, _)| n == path) {
            *isi = xml_baru.into_bytes();
        }
    }
    let styles_baru = gaya.tulis(&styles);
    if styles_baru != styles {
        if let Some((_, isi)) = berkas.iter_mut().find(|(n, _)| n == "xl/styles.xml") {
            *isi = styles_baru.into_bytes();
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

/// Referensi sel yang berisi rumus di satu sheet template.
pub fn sel_rumus(template: &[u8], sheet: &str) -> Result<HashSet<String>, String> {
    let mut zin = zip::ZipArchive::new(Cursor::new(template)).map_err(|e| e.to_string())?;
    let mut ambil = |nama: &str| -> Result<String, String> {
        let mut s = String::new();
        zin.by_name(nama).map_err(|e| format!("{nama}: {e}"))?.read_to_string(&mut s).map_err(|e| e.to_string())?;
        Ok(s)
    };
    let peta = peta_sheet(&ambil("xl/workbook.xml")?, &ambil("xl/_rels/workbook.xml.rels")?)?;
    let path = peta.get(sheet).ok_or_else(|| format!("Sheet '{sheet}' tidak ada di template"))?;
    let xml = ambil(path)?;
    let re_sel = Regex::new(r#"<c r="([A-Z]+[0-9]+)"[^>]*?(?:/>|>(.*?)</c>)"#).unwrap();
    Ok(re_sel
        .captures_iter(&xml)
        .filter(|c| c.get(2).is_some_and(|m| m.as_str().contains("<f")))
        .map(|c| c[1].to_string())
        .collect())
}
