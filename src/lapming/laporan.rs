//! Penulis laporan bulanan Korwil (.docx). Dibuat langsung sebagai OOXML, tanpa library tambahan.
//! Teks yang diapit `{{` dan `}}` dicetak merah: tandanya data yang belum ada atau perlu dikonfirmasi.

use std::io::{Cursor, Write};

use zip::{write::SimpleFileOptions, ZipWriter};

/// Satu blok isi laporan.
pub enum Blok {
    /// Judul rata tengah, tebal. Ukuran dalam half-point (28 = 14 pt).
    Judul(String, u32),
    /// Paragraf rata kiri-kanan.
    Paragraf(String),
    /// Subjudul tebal rata kiri.
    Sub(String),
    /// Tabel dengan baris header. `lebar` dalam dxa (total 9026); kosong = dibagi rata.
    Tabel { header: Vec<String>, baris: Vec<Vec<String>>, lebar: Vec<u32> },
    /// Pindah halaman.
    Halaman,
}

/// Escape untuk teks XML.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Pecah teks menjadi potongan biasa dan potongan merah (`{{...}}`).
fn runs(teks: &str, tebal: bool, ukuran: u32) -> String {
    let mut out = String::new();
    let mut sisa = teks;
    loop {
        match sisa.find("{{") {
            Some(a) => {
                if a > 0 {
                    out.push_str(&run(&sisa[..a], tebal, ukuran, false));
                }
                let setelah = &sisa[a + 2..];
                match setelah.find("}}") {
                    Some(b) => {
                        out.push_str(&run(&setelah[..b], tebal, ukuran, true));
                        sisa = &setelah[b + 2..];
                    }
                    None => {
                        out.push_str(&run(&sisa[a..], tebal, ukuran, false));
                        break;
                    }
                }
            }
            None => {
                if !sisa.is_empty() {
                    out.push_str(&run(sisa, tebal, ukuran, false));
                }
                break;
            }
        }
    }
    out
}

fn run(teks: &str, tebal: bool, ukuran: u32, merah: bool) -> String {
    let mut rpr = String::from(r#"<w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:cs="Times New Roman"/>"#);
    if tebal {
        rpr.push_str("<w:b/>");
    }
    if merah {
        rpr.push_str(r#"<w:color w:val="FF0000"/>"#);
    }
    rpr.push_str(&format!(r#"<w:sz w:val="{ukuran}"/><w:szCs w:val="{ukuran}"/>"#));
    format!(r#"<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#, esc(teks))
}

fn paragraf(teks: &str, jc: &str, tebal: bool, ukuran: u32) -> String {
    format!(
        r#"<w:p><w:pPr><w:spacing w:after="120" w:line="276" w:lineRule="auto"/><w:jc w:val="{jc}"/></w:pPr>{}</w:p>"#,
        runs(teks, tebal, ukuran)
    )
}

fn tabel(header: &[String], baris: &[Vec<String>], lebar: &[u32]) -> String {
    let jumlah = header.len().max(1);
    let lebar: Vec<u32> = if lebar.len() == jumlah { lebar.to_vec() } else { vec![9026 / jumlah as u32; jumlah] };
    let batas: String = ["top", "left", "bottom", "right", "insideH", "insideV"]
        .iter()
        .map(|sisi| format!(r#"<w:{sisi} w:val="single" w:sz="4" w:space="0" w:color="999999"/>"#))
        .collect();
    let grid: String = lebar.iter().map(|w| format!(r#"<w:gridCol w:w="{w}"/>"#)).collect();
    let sel = |teks: &str, w: u32, kepala: bool| {
        let shd = if kepala { r#"<w:shd w:val="clear" w:color="auto" w:fill="BDD7EE"/>"# } else { "" };
        format!(
            r#"<w:tc><w:tcPr><w:tcW w:w="{w}" w:type="dxa"/>{shd}</w:tcPr><w:p><w:pPr><w:spacing w:after="0"/></w:pPr>{}</w:p></w:tc>"#,
            runs(teks, kepala, 18)
        )
    };
    let mut out = format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="9026" w:type="dxa"/><w:tblBorders>{batas}</w:tblBorders><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid>{grid}</w:tblGrid>"#
    );
    out.push_str(r#"<w:tr><w:trPr><w:tblHeader/></w:trPr>"#);
    for (i, h) in header.iter().enumerate() {
        out.push_str(&sel(h, lebar[i], true));
    }
    out.push_str("</w:tr>");
    for b in baris {
        out.push_str("<w:tr>");
        for (i, c) in b.iter().enumerate().take(jumlah) {
            out.push_str(&sel(c, lebar[i], false));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    // Paragraf kosong setelah tabel, supaya tabel berikutnya tidak menempel.
    out.push_str(r#"<w:p><w:pPr><w:spacing w:after="120"/></w:pPr></w:p>"#);
    out
}

/// Menyusun berkas .docx dari daftar blok.
pub fn tulis_docx(blok: &[Blok]) -> Result<Vec<u8>, String> {
    let mut body = String::new();
    for b in blok {
        match b {
            Blok::Judul(t, u) => body.push_str(&paragraf(t, "center", true, *u)),
            Blok::Paragraf(t) => body.push_str(&paragraf(t, "both", false, 24)),
            Blok::Sub(t) => body.push_str(&paragraf(t, "left", true, 24)),
            Blok::Tabel { header, baris, lebar } => body.push_str(&tabel(header, baris, lebar)),
            Blok::Halaman => body.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#),
        }
    }
    let dokumen = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>{body}<w:sectPr><w:footerReference w:type="default" r:id="rId2"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    // Nomor halaman di kaki halaman (field PAGE).
    const FOOTER: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:sz w:val="20"/></w:rPr><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:rPr><w:sz w:val="20"/></w:rPr><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:rPr><w:sz w:val="20"/></w:rPr><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:rPr><w:sz w:val="20"/></w:rPr><w:t>1</w:t></w:r><w:r><w:rPr><w:sz w:val="20"/></w:rPr><w:fldChar w:fldCharType="end"/></w:r></w:p></w:ftr>"#;
    const REL_DOKUMEN: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#;
    const TIPE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#;
    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opsi = SimpleFileOptions::default();
    for (nama, isi) in [
        ("[Content_Types].xml", TIPE.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("word/document.xml", dokumen.as_bytes()),
        ("word/_rels/document.xml.rels", REL_DOKUMEN.as_bytes()),
        ("word/footer1.xml", FOOTER.as_bytes()),
    ] {
        zip.start_file(nama, opsi).map_err(|e| e.to_string())?;
        zip.write_all(isi).map_err(|e| e.to_string())?;
    }
    let cur = zip.finish().map_err(|e| e.to_string())?;
    Ok(cur.into_inner())
}
