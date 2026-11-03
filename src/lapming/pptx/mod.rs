//! Penulis PPTX (OOXML) sederhana: bentuk, teks, tabel, dan chart. Meniru keluaran python-pptx
//! supaya tampilannya sama dengan skrip generate_lapming.py. Kerangka (master, layout, tema)
//! diambil dari template.pptx.

pub mod chart;
pub mod slides;

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use chart::ChartSpec;

pub const FONT: &str = "Segoe UI";
const TEMPLATE: &[u8] = include_bytes!("template.pptx");

/// Titik (pt) ke EMU, dipotong ke bawah seperti `int(points * 12700)` di python-pptx.
pub fn emu(pt: f64) -> i64 {
    (pt * 12700.0) as i64
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    L,
    C,
    R,
}

impl Align {
    fn kode(self) -> &'static str {
        match self {
            Align::L => "l",
            Align::C => "ctr",
            Align::R => "r",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Anchor {
    Top,
    Middle,
    Bottom,
}

impl Anchor {
    fn kode(self) -> &'static str {
        match self {
            Anchor::Top => "t",
            Anchor::Middle => "ctr",
            Anchor::Bottom => "b",
        }
    }
}

/// Satu potongan teks dengan override ukuran, tebal, dan warna.
#[derive(Clone, Default)]
pub struct Run {
    pub t: String,
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub color: Option<String>,
}

impl Run {
    pub fn new(t: impl Into<String>) -> Self {
        Run { t: t.into(), ..Default::default() }
    }
    pub fn bold(mut self) -> Self {
        self.bold = Some(true);
        self
    }
    pub fn size(mut self, s: f64) -> Self {
        self.size = Some(s);
        self
    }
    pub fn color(mut self, c: &str) -> Self {
        self.color = Some(c.to_string());
        self
    }
}

/// Satu paragraf: potongan teks, poin (bullet), dan jarak sebelum paragraf.
#[derive(Clone, Default)]
pub struct Para {
    pub runs: Vec<Run>,
    pub bullet: bool,
    pub space: Option<f64>,
}

impl Para {
    pub fn txt(t: impl Into<String>) -> Self {
        Para { runs: vec![Run::new(t)], ..Default::default() }
    }
    pub fn runs(runs: Vec<Run>) -> Self {
        Para { runs, ..Default::default() }
    }
    pub fn bullet(mut self) -> Self {
        self.bullet = true;
        self
    }
    pub fn space(mut self, s: f64) -> Self {
        self.space = Some(s);
        self
    }
    /// Judul kecil: tebal, dengan warna dan jarak.
    pub fn judul(t: impl Into<String>, color: &str, space: f64) -> Self {
        Para { runs: vec![Run::new(t).bold().color(color)], space: Some(space), ..Default::default() }
    }
}

#[derive(Clone)]
pub struct TextOpts {
    pub size: f64,
    pub color: String,
    pub bold: bool,
    pub align: Align,
    pub anchor: Anchor,
    pub inset: f64,
}

impl TextOpts {
    pub fn new(size: f64) -> Self {
        TextOpts { size, color: slides::TEXT.to_string(), bold: false, align: Align::L, anchor: Anchor::Top, inset: 0.0 }
    }
    pub fn color(mut self, c: &str) -> Self {
        self.color = c.to_string();
        self
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    pub fn anchor(mut self, a: Anchor) -> Self {
        self.anchor = a;
        self
    }
}

const STYLE_SHAPE: &str = "<p:style><a:lnRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:lnRef><a:fillRef idx=\"3\"><a:schemeClr val=\"accent1\"/></a:fillRef><a:effectRef idx=\"2\"><a:schemeClr val=\"accent1\"/></a:effectRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></p:style>";
const STYLE_LINE: &str = "<p:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent1\"/></a:lnRef><a:fillRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:fillRef><a:effectRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:effectRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"tx1\"/></a:fontRef></p:style>";

fn rpr(size: f64, bold: bool, color: &str) -> String {
    format!(
        "<a:rPr sz=\"{}\" b=\"{}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:latin typeface=\"{}\"/></a:rPr>",
        (size * 100.0).round() as i64,
        if bold { 1 } else { 0 },
        color,
        FONT
    )
}

/// Satu slide: latar dan daftar bentuk dalam bentuk XML. Chart disimpan terpisah.
pub struct Slide {
    bg: String,
    shapes: Vec<String>,
    next_id: u32,
    charts: Vec<ChartSpec>,
}

/// Konfigurasi tabel (setara argumen table() di skrip Python).
pub struct TableOpts<'a> {
    pub widths: &'a [f64],
    pub num_cols: HashSet<usize>,
    pub tint: HashMap<usize, (&'static str, &'static str)>,
    pub size: f64,
    pub row_h: f64,
    pub bold_cols: HashSet<usize>,
    pub total: bool,
    pub grup: HashSet<usize>,
    /// Sel yang berwarna merah (baris, kolom).
    pub merah: HashSet<(usize, usize)>,
}

impl Slide {
    pub fn new(bg: &str) -> Slide {
        Slide { bg: bg.to_string(), shapes: Vec::new(), next_id: 1, charts: Vec::new() }
    }

    /// Jumlah slide dihitung oleh pemanggil; ini jumlah bentuk saat ini.
    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn xfrm(x: f64, y: f64, w: f64, h: f64) -> String {
        format!("<a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>", emu(x), emu(y), emu(w), emu(h))
    }

    /// Persegi atau persegi bulat (adj dalam pecahan, mis. 0.06) tanpa garis tepi.
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: &str, adj: Option<f64>) {
        let id = self.id();
        let (nama, geom) = match adj {
            Some(a) => (
                "Rounded Rectangle",
                format!("<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val {}\"/></a:avLst></a:prstGeom>", (a * 100000.0).round() as i64),
            ),
            None => ("Rectangle", "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_string()),
        };
        self.shapes.push(format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{nama} {}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{geom}<a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr>{STYLE_SHAPE}<p:txBody><a:bodyPr rtlCol=\"0\" anchor=\"ctr\"/><a:lstStyle/><a:p><a:pPr algn=\"ctr\"/></a:p></p:txBody></p:sp>",
            id - 1,
            Self::xfrm(x, y, w, h)
        ));
    }

    pub fn text(&mut self, x: f64, y: f64, w: f64, h: f64, paras: Vec<Para>, o: &TextOpts) {
        let id = self.id();
        let mut p_xml = String::new();
        for p in &paras {
            let mut ppr = format!("<a:pPr algn=\"{}\"", o.align.kode());
            if p.bullet {
                ppr.push_str(" marL=\"171450\" indent=\"-171450\"");
            }
            ppr.push('>');
            if let Some(s) = p.space {
                ppr.push_str(&format!("<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>", (s * 100.0).round() as i64));
            }
            if p.bullet {
                ppr.push_str("<a:buChar char=\"•\"/>");
            }
            ppr.push_str("</a:pPr>");
            let mut runs = String::new();
            for r in &p.runs {
                runs.push_str(&format!(
                    "<a:r>{}<a:t>{}</a:t></a:r>",
                    rpr(r.size.unwrap_or(o.size), r.bold.unwrap_or(o.bold), r.color.as_deref().unwrap_or(&o.color)),
                    esc(&r.t)
                ));
            }
            p_xml.push_str(&format!("<a:p>{ppr}{runs}</a:p>"));
        }
        let ins = emu(o.inset);
        self.shapes.push(format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"TextBox {}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" anchor=\"{}\" lIns=\"{ins}\" rIns=\"{ins}\" tIns=\"{ins}\" bIns=\"{ins}\"><a:spAutoFit/></a:bodyPr><a:lstStyle/>{p_xml}</p:txBody></p:sp>",
            id - 1,
            Self::xfrm(x, y, w, h),
            o.anchor.kode()
        ));
    }

    /// Teks satu paragraf.
    pub fn teks(&mut self, x: f64, y: f64, w: f64, h: f64, t: &str, o: &TextOpts) {
        self.text(x, y, w, h, vec![Para::txt(t)], o);
    }

    pub fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: &str, width: Option<f64>) {
        let id = self.id();
        let ln = match width {
            Some(w) => format!("<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill></a:ln>", emu(w)),
            None => format!("<a:ln><a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill></a:ln>"),
        };
        self.shapes.push(format!(
            "<p:cxnSp><p:nvCxnSpPr><p:cNvPr id=\"{id}\" name=\"Connector {}\"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>{}<a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom>{ln}</p:spPr>{STYLE_LINE}</p:cxnSp>",
            id - 1,
            Self::xfrm(x1, y1, x2 - x1, y2 - y1)
        ));
    }

    /// Menambah chart. Mengembalikan indeks chart pada slide ini.
    pub fn chart(&mut self, x: f64, y: f64, w: f64, h: f64, spec: ChartSpec) {
        let id = self.id();
        self.charts.push(spec);
        let rid = self.charts.len() + 1; // rId1 = layout
        self.shapes.push(format!(
            "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Chart {}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"rId{rid}\"/></a:graphicData></a:graphic></p:graphicFrame>",
            id - 1,
            emu(x),
            emu(y),
            emu(w),
            emu(h)
        ));
    }

    /// Tabel tanpa garis tegak: header bergaris tebal, baris pembatas (grup) digabung, baris total opsional.
    pub fn table(&mut self, x: f64, y: f64, w: f64, data: &[Vec<String>], o: &TableOpts) {
        let id = self.id();
        let nr = data.len();
        let nc = data[0].len();
        let tot_w: f64 = o.widths.iter().sum();
        let mut grid = String::new();
        for cw in o.widths {
            grid.push_str(&format!("<a:gridCol w=\"{}\"/>", emu(w * cw / tot_w)));
        }
        let mut rows = String::new();
        for (i, baris) in data.iter().enumerate() {
            let h = o.row_h + if i == 0 { 8.0 } else { 0.0 };
            rows.push_str(&format!("<a:tr h=\"{}\">", emu(h)));
            for j in 0..nc {
                let teks = baris.get(j).map(String::as_str).unwrap_or("");
                let grup = o.grup.contains(&i);
                let kanan = o.num_cols.contains(&j) && !grup;
                let tebal = i == 0 || o.bold_cols.contains(&j) || (o.total && i == nr - 1) || grup;
                let mut warna = slides::TEXT.to_string();
                let tint = if !grup { o.tint.get(&j) } else { None };
                if o.merah.contains(&(i, j)) {
                    warna = slides::MERAH.to_string();
                }
                if grup {
                    warna = slides::NAVY.to_string();
                } else if let Some((tc, _)) = tint {
                    if i == 0 {
                        warna = tc.to_string();
                    }
                }
                let fill = if grup {
                    format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", slides::CARD)
                } else if let Some((_, bgc)) = tint {
                    format!("<a:solidFill><a:srgbClr val=\"{bgc}\"/></a:solidFill>")
                } else {
                    "<a:noFill/>".to_string()
                };
                let mut tepi = String::new();
                for tag in ["lnL", "lnR", "lnT", "lnB"] {
                    let head = i == 0 && tag == "lnB";
                    let tot_line = o.total && i == nr - 1 && tag == "lnT";
                    let wl = if head || tot_line { 19050 } else { 6350 };
                    if tag == "lnB" || tot_line {
                        let c = if head || tot_line { slides::NAVY } else { slides::RULE };
                        tepi.push_str(&format!("<a:{tag} w=\"{wl}\"><a:solidFill><a:srgbClr val=\"{c}\"/></a:solidFill></a:{tag}>"));
                    } else {
                        tepi.push_str(&format!("<a:{tag} w=\"{wl}\"><a:noFill/></a:{tag}>"));
                    }
                }
                let tc_attr = if grup && j == 0 {
                    format!(" gridSpan=\"{nc}\"")
                } else if grup {
                    " hMerge=\"1\"".to_string()
                } else {
                    String::new()
                };
                let ukuran = o.size - if i == 0 { 1.0 } else { 0.0 };
                rows.push_str(&format!(
                    "<a:tc{tc_attr}><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr algn=\"{}\"/><a:r>{}<a:t>{}</a:t></a:r></a:p></a:txBody><a:tcPr marL=\"50800\" marR=\"50800\" marT=\"12700\" marB=\"12700\" anchor=\"{}\">{tepi}{fill}</a:tcPr></a:tc>",
                    if kanan { "r" } else { "l" },
                    rpr(ukuran, tebal, &warna),
                    esc(teks),
                    if i == 0 { "b" } else { "ctr" }
                ));
            }
            rows.push_str("</a:tr>");
        }
        self.shapes.push(format!(
            "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr firstRow=\"0\" bandRow=\"0\"><a:tableStyleId>{{2D5ABB26-0587-4C30-8999-92F81FD0307C}}</a:tableStyleId></a:tblPr><a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
            id - 1,
            emu(x),
            emu(y),
            emu(w),
            emu(o.row_h * nr as f64)
        ));
    }

    fn xml(&self) -> String {
        format!(
            "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\n<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
            self.bg,
            self.shapes.concat()
        )
    }
}

/// Kumpulan slide yang akan ditulis menjadi satu berkas pptx.
#[derive(Default)]
pub struct Deck {
    pub slides: Vec<Slide>,
    pub judul: String,
}

impl Deck {
    pub fn tambah(&mut self, bg: &str) -> &mut Slide {
        self.slides.push(Slide::new(bg));
        self.slides.last_mut().unwrap()
    }

    pub fn jumlah(&self) -> usize {
        self.slides.len()
    }

    /// Menulis berkas pptx lengkap: kerangka dari template, ditambah slide, chart, dan workbook tertanam.
    pub fn simpan(&self) -> Result<Vec<u8>, String> {
        let mut zin = ZipArchive::new(Cursor::new(TEMPLATE)).map_err(|e| e.to_string())?;
        let mut out = ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        // Nomor chart global, berurutan menurut slide.
        let mut chart_no = 0usize;
        let mut slide_xml: Vec<(String, String)> = Vec::new();
        let mut chart_parts: Vec<(usize, String, Vec<u8>)> = Vec::new();
        for (si, s) in self.slides.iter().enumerate() {
            let mut rels = String::from("<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout7.xml\"/>");
            for (ci, c) in s.charts.iter().enumerate() {
                chart_no += 1;
                rels.push_str(&format!(
                    "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart\" Target=\"../charts/chart{chart_no}.xml\"/>",
                    ci + 2
                ));
                chart_parts.push((chart_no, c.xml(), c.workbook()?));
            }
            rels.push_str("</Relationships>");
            slide_xml.push((format!("ppt/slides/slide{}.xml", si + 1), s.xml()));
            slide_xml.push((format!("ppt/slides/_rels/slide{}.xml.rels", si + 1), rels));
        }

        for i in 0..zin.len() {
            let mut f = zin.by_index(i).map_err(|e| e.to_string())?;
            let nama = f.name().to_string();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            let buf = match nama.as_str() {
                "[Content_Types].xml" => self.content_types(&String::from_utf8_lossy(&buf), chart_no).into_bytes(),
                "ppt/presentation.xml" => self.presentation(&String::from_utf8_lossy(&buf)).into_bytes(),
                "ppt/_rels/presentation.xml.rels" => self.presentation_rels(&String::from_utf8_lossy(&buf)).into_bytes(),
                "docProps/core.xml" => self.core().into_bytes(),
                "docProps/thumbnail.jpeg" => continue,
                "_rels/.rels" => regex::Regex::new(r"<Relationship [^>]*thumbnail[^>]*/>")
                    .unwrap()
                    .replace_all(&String::from_utf8_lossy(&buf), "")
                    .into_owned()
                    .into_bytes(),
                _ => buf,
            };
            out.start_file(nama, opt).map_err(|e| e.to_string())?;
            out.write_all(&buf).map_err(|e| e.to_string())?;
        }
        for (nama, xml) in slide_xml {
            out.start_file(nama, opt).map_err(|e| e.to_string())?;
            out.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
        }
        for (n, xml, wb) in chart_parts {
            out.start_file(format!("ppt/charts/chart{n}.xml"), opt).map_err(|e| e.to_string())?;
            out.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
            out.start_file(format!("ppt/charts/_rels/chart{n}.xml.rels"), opt).map_err(|e| e.to_string())?;
            out.write_all(
                format!("<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/package\" Target=\"../embeddings/Microsoft_Excel_Sheet{n}.xlsx\"/></Relationships>").as_bytes(),
            )
            .map_err(|e| e.to_string())?;
            out.start_file(format!("ppt/embeddings/Microsoft_Excel_Sheet{n}.xlsx"), opt).map_err(|e| e.to_string())?;
            out.write_all(&wb).map_err(|e| e.to_string())?;
        }
        let cur = out.finish().map_err(|e| e.to_string())?;
        Ok(cur.into_inner())
    }

    fn content_types(&self, asli: &str, n_chart: usize) -> String {
        let mut extra = String::new();
        if !asli.contains("Extension=\"xlsx\"") {
            extra.push_str("<Default Extension=\"xlsx\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\"/>");
        }
        let mut ov = String::new();
        for i in 1..=self.slides.len() {
            ov.push_str(&format!("<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"));
        }
        for i in 1..=n_chart {
            ov.push_str(&format!("<Override PartName=\"/ppt/charts/chart{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\"/>"));
        }
        let mut s = asli.replace("<Default Extension=\"rels\"", &format!("{extra}<Default Extension=\"rels\""));
        s = s.replace("</Types>", &format!("{ov}</Types>"));
        s.replace("<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>", "")
    }

    fn presentation(&self, asli: &str) -> String {
        let mut ids = String::from("<p:sldIdLst>");
        for i in 0..self.slides.len() {
            ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 256 + i, 7 + i));
        }
        ids.push_str("</p:sldIdLst>");
        asli.replace("</p:sldMasterIdLst>", &format!("</p:sldMasterIdLst>{ids}"))
    }

    fn presentation_rels(&self, asli: &str) -> String {
        let mut r = String::new();
        for i in 0..self.slides.len() {
            r.push_str(&format!(
                "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{}.xml\"/>",
                7 + i,
                i + 1
            ));
        }
        asli.replace("</Relationships>", &format!("{r}</Relationships>"))
    }

    fn core(&self) -> String {
        let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        format!(
            "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>KorwilAPP</dc:creator><cp:lastModifiedBy>KorwilAPP</cp:lastModifiedBy><cp:revision>1</cp:revision><dcterms:created xsi:type=\"dcterms:W3CDTF\">{ts}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{ts}</dcterms:modified></cp:coreProperties>",
            esc(&self.judul)
        )
    }
}