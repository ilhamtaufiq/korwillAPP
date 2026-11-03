//! Chart batang (horizontal, kolom, bertumpuk) dalam OOXML, mengikuti keluaran python-pptx.

use rust_xlsxwriter::{Workbook, XlsxError};

use super::{esc, FONT};

const NAVY_RULE: &str = "DDE2EE";
const MUTED: &str = "5F6B85";
const TEXT: &str = "1B2540";
const AX_CAT: &str = "-2068027336";
const AX_VAL: &str = "-2113994440";

#[derive(Clone)]
pub struct Label {
    pub fmt: String,
    pub size: f64,
    pub bold: bool,
    pub color: String,
    pub pos: &'static str,
    /// Putar teks label vertikal supaya tidak bertumpuk pada chart dengan banyak kategori.
    pub rotate: bool,
}

#[derive(Clone)]
pub struct PointLabel {
    pub idx: usize,
    pub text: String,
    pub color: String,
    pub size: f64,
    pub bold: bool,
    pub pos: Option<&'static str>,
}

#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub vals: Vec<f64>,
    /// None = tanpa isi (dipakai untuk seri total yang hanya membawa label).
    pub color: Option<String>,
    pub label: Option<Label>,
    pub pt_colors: Vec<(usize, String)>,
    pub pt_labels: Vec<PointLabel>,
}

impl Series {
    pub fn new(name: &str, vals: Vec<f64>, color: &str) -> Series {
        Series {
            name: name.to_string(),
            vals,
            color: Some(color.to_string()),
            label: None,
            pt_colors: Vec::new(),
            pt_labels: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct ChartSpec {
    /// true = kolom (vertikal), false = batang horizontal.
    pub col: bool,
    pub stacked: bool,
    pub cats: Vec<String>,
    pub series: Vec<Series>,
    pub gap: u32,
    pub overlap: Option<i32>,
    /// (teks, ukuran, tebal)
    pub title: Option<(String, f64, bool)>,
    /// Ukuran font legenda; None = tanpa legenda.
    pub legend: Option<f64>,
    pub reverse_cats: bool,
    pub cat_low: bool,
    pub val_visible: bool,
    pub val_min: Option<f64>,
    pub val_max: Option<f64>,
    pub val_fmt: Option<String>,
    /// Ukuran font label sumbu nilai; kosong = ukuran chart dikurangi 1.
    pub val_size: Option<f64>,
    pub size: f64,
}

impl ChartSpec {
    /// Batang horizontal; kategori pertama tampil di atas.
    pub fn bar(cats: Vec<String>, series: Vec<Series>, size: f64) -> ChartSpec {
        ChartSpec {
            col: false,
            stacked: false,
            cats,
            series,
            gap: 45,
            overlap: None,
            title: None,
            legend: None,
            reverse_cats: true,
            cat_low: false,
            val_visible: false,
            val_min: None,
            val_max: None,
            val_fmt: None,
            val_size: None,
            size,
        }
    }

    fn rpr_run(size: f64, bold: bool, color: &str) -> String {
        format!(
            "<a:rPr sz=\"{}\" b=\"{}\"><a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill><a:latin typeface=\"{FONT}\"/></a:rPr>",
            (size * 100.0).round() as i64,
            if bold { 1 } else { 0 }
        )
    }

    fn tx_pr(size: f64, bold: bool, color: &str) -> String {
        Self::tx_pr_rot(size, bold, color, false)
    }

    fn tx_pr_rot(size: f64, bold: bool, color: &str, rotate: bool) -> String {
        let body = if rotate { "<a:bodyPr rot=\"-5400000\" vert=\"horz\"/>" } else { "<a:bodyPr/>" };
        format!(
            "<c:txPr>{body}<a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"{}\"{}><a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill></a:defRPr></a:pPr></a:p></c:txPr>",
            (size * 100.0).round() as i64,
            if bold { " b=\"1\"" } else { "" }
        )
    }

    fn kol(i: usize) -> String {
        ((b'B' + i as u8) as char).to_string()
    }

    fn cat_xml(&self) -> String {
        let n = self.cats.len();
        let mut pts = String::new();
        for (i, c) in self.cats.iter().enumerate() {
            pts.push_str(&format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(c)));
        }
        format!("<c:cat><c:strRef><c:f>Sheet1!$A$2:$A${}</c:f><c:strCache><c:ptCount val=\"{n}\"/>{pts}</c:strCache></c:strRef></c:cat>", n + 1)
    }

    fn ser_xml(&self, i: usize, s: &Series) -> String {
        let kol = Self::kol(i);
        let n = s.vals.len();
        let mut x = format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:strRef><c:f>Sheet1!${kol}$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx>",
            esc(&s.name)
        );
        match &s.color {
            Some(c) => x.push_str(&format!("<c:spPr><a:solidFill><a:srgbClr val=\"{c}\"/></a:solidFill></c:spPr>")),
            None => x.push_str("<c:spPr><a:noFill/></c:spPr>"),
        }
        x.push_str("<c:invertIfNegative val=\"0\"/>");
        for (idx, c) in &s.pt_colors {
            x.push_str(&format!("<c:dPt><c:idx val=\"{idx}\"/><c:spPr><a:solidFill><a:srgbClr val=\"{c}\"/></a:solidFill></c:spPr></c:dPt>"));
        }
        if s.label.is_some() || !s.pt_labels.is_empty() {
            x.push_str("<c:dLbls>");
            for p in &s.pt_labels {
                x.push_str(&format!(
                    "<c:dLbl><c:idx val=\"{}\"/><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r>{}<a:t>{}</a:t></a:r></a:p></c:rich></c:tx>{}<c:showLegendKey val=\"0\"/><c:showVal val=\"1\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"0\"/><c:showBubbleSize val=\"0\"/></c:dLbl>",
                    p.idx,
                    Self::rpr_run(p.size, p.bold, &p.color),
                    esc(&p.text),
                    p.pos.map(|v| format!("<c:dLblPos val=\"{v}\"/>")).unwrap_or_default()
                ));
            }
            match &s.label {
                Some(l) => x.push_str(&format!(
                    "<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>{}<c:dLblPos val=\"{}\"/><c:showLegendKey val=\"0\"/><c:showVal val=\"1\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"0\"/><c:showBubbleSize val=\"0\"/><c:showLeaderLines val=\"1\"/>",
                    esc(&l.fmt).replace('"', "&quot;"),
                    Self::tx_pr_rot(l.size, l.bold, &l.color, l.rotate),
                    l.pos
                )),
                None => x.push_str("<c:showLegendKey val=\"0\"/><c:showVal val=\"0\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"0\"/><c:showBubbleSize val=\"0\"/><c:showLeaderLines val=\"1\"/>"),
            }
            x.push_str("</c:dLbls>");
        }
        x.push_str(&self.cat_xml());
        let mut pts = String::new();
        for (j, v) in s.vals.iter().enumerate() {
            pts.push_str(&format!("<c:pt idx=\"{j}\"><c:v>{v}</c:v></c:pt>"));
        }
        x.push_str(&format!(
            "<c:val><c:numRef><c:f>Sheet1!${kol}$2:${kol}${}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{n}\"/>{pts}</c:numCache></c:numRef></c:val></c:ser>",
            n + 1
        ));
        x
    }

    pub fn xml(&self) -> String {
        let mut x = String::from("<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\n<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:date1904 val=\"0\"/><c:chart>");
        match &self.title {
            Some((t, size, bold)) => x.push_str(&format!(
                "<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r>{}<a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:layout/><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>",
                Self::rpr_run(*size, *bold, TEXT),
                esc(t)
            )),
            None => x.push_str("<c:autoTitleDeleted val=\"1\"/>"),
        }
        x.push_str(&format!(
            "<c:plotArea><c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"{}\"/>",
            if self.col { "col" } else { "bar" },
            if self.stacked { "stacked" } else { "clustered" }
        ));
        for (i, s) in self.series.iter().enumerate() {
            x.push_str(&self.ser_xml(i, s));
        }
        x.push_str(&format!("<c:gapWidth val=\"{}\"/>", self.gap));
        if let Some(o) = self.overlap {
            x.push_str(&format!("<c:overlap val=\"{o}\"/>"));
        }
        x.push_str(&format!("<c:axId val=\"{AX_CAT}\"/><c:axId val=\"{AX_VAL}\"/></c:barChart>"));

        // Sumbu kategori
        x.push_str(&format!(
            "<c:catAx><c:axId val=\"{AX_CAT}\"/><c:scaling><c:orientation val=\"{}\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{}\"/><c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"{}\"/><c:spPr><a:ln><a:solidFill><a:srgbClr val=\"{NAVY_RULE}\"/></a:solidFill></a:ln></c:spPr>{}<c:crossAx val=\"{AX_VAL}\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx>",
            if self.reverse_cats { "maxMin" } else { "minMax" },
            if self.col { "b" } else { "l" },
            if self.cat_low { "low" } else { "nextTo" },
            Self::tx_pr(self.size, false, TEXT)
        ));
        // Sumbu nilai
        let mut skala = String::new();
        if let Some(m) = self.val_max {
            skala.push_str(&format!("<c:max val=\"{m}\"/>"));
        }
        if let Some(m) = self.val_min {
            skala.push_str(&format!("<c:min val=\"{m}\"/>"));
        }
        let pos = if self.col { "l" } else { "b" };
        if self.val_visible {
            x.push_str(&format!(
                "<c:valAx><c:axId val=\"{AX_VAL}\"/><c:scaling>{skala}</c:scaling><c:delete val=\"0\"/><c:axPos val=\"{pos}\"/><c:majorGridlines><c:spPr><a:ln><a:solidFill><a:srgbClr val=\"{NAVY_RULE}\"/></a:solidFill></a:ln></c:spPr></c:majorGridlines>{}<c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:spPr><a:ln><a:noFill/></a:ln></c:spPr>{}<c:crossAx val=\"{AX_CAT}\"/><c:crosses val=\"autoZero\"/></c:valAx>",
                self.val_fmt
                    .as_ref()
                    .map(|f| format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>", esc(f).replace('"', "&quot;")))
                    .unwrap_or_default(),
                Self::tx_pr(self.val_size.unwrap_or(self.size - 1.0), false, MUTED)
            ));
        } else {
            x.push_str(&format!(
                "<c:valAx><c:axId val=\"{AX_VAL}\"/><c:scaling>{skala}</c:scaling><c:delete/><c:axPos val=\"{pos}\"/><c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"{AX_CAT}\"/><c:crosses val=\"autoZero\"/></c:valAx>"
            ));
        }
        x.push_str("</c:plotArea>");
        if let Some(ls) = self.legend {
            x.push_str(&format!(
                "<c:legend><c:legendPos val=\"t\"/><c:overlay val=\"0\"/>{}</c:legend>",
                Self::tx_pr(ls, false, TEXT)
            ));
        }
        x.push_str(&format!(
            "<c:dispBlanksAs val=\"gap\"/></c:chart><c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"{}\"><a:solidFill><a:srgbClr val=\"{TEXT}\"/></a:solidFill><a:latin typeface=\"{FONT}\"/></a:defRPr></a:pPr><a:endParaRPr lang=\"en-US\"/></a:p></c:txPr><c:externalData r:id=\"rId1\"><c:autoUpdate val=\"0\"/></c:externalData></c:chartSpace>",
            (self.size * 100.0).round() as i64
        ));
        x
    }

    /// Workbook tertanam: kategori di kolom A, setiap seri di kolom berikutnya (supaya data chart bisa diedit).
    pub fn workbook(&self) -> Result<Vec<u8>, String> {
        let f = || -> Result<Vec<u8>, XlsxError> {
            let mut wb = Workbook::new();
            let ws = wb.add_worksheet();
            ws.set_name("Sheet1")?;
            for (i, s) in self.series.iter().enumerate() {
                ws.write_string(0, (i + 1) as u16, &s.name)?;
                for (j, v) in s.vals.iter().enumerate() {
                    ws.write_number((j + 1) as u32, (i + 1) as u16, *v)?;
                }
            }
            for (j, c) in self.cats.iter().enumerate() {
                ws.write_string((j + 1) as u32, 0, c)?;
            }
            wb.save_to_buffer()
        };
        f().map_err(|e| e.to_string())
    }
}