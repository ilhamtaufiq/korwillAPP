//! Penulis xlsx gabungan Sanimas + LPK untuk satu korfas (port tulis_gabungan dari generate_lapming.py).
//! Sheet: M1..Mn, Kumulatif (rumus, pilih minggu di C3), Anomali, Cek Data, Log Perubahan.

use std::collections::HashMap;

use chrono::{Local, NaiveDate};
use rust_xlsxwriter::utility::column_number_to_name;
use rust_xlsxwriter::{Color, DataValidation, Format, FormatAlign, FormatBorder, Formula, Workbook, Worksheet, XlsxError};

use super::calc::{ambil, periode_sheet, Laporan};
use super::fmt::norm_nama;
use super::log::{fmt_val, LogEntri};
use super::model::{field_nama, Dataset, Row};

const HIJAU: u32 = 0x223468;
const KUNING: u32 = 0xFFF2CC;
const ABU: u32 = 0xEEF1F8;

/// (judul, kunci, format angka, lebar) untuk sheet minggu.
const KOL_MINGGU: [(&str, &str, &str, f64); 28] = [
    ("No", "_i", "0", 5.0),
    ("Jenis", "_jenis", "@", 9.0),
    ("Kabupaten/Kota", "kab", "@", 14.0),
    ("Kecamatan", "kec", "@", 16.0),
    ("Desa/Kelurahan", "desa", "@", 18.0),
    ("Nama Pokmas", "_pokmas", "@", 34.0),
    ("Korfas", "korfas", "@", 12.0),
    ("Tahap", "tahap", "@", 9.0),
    ("Pagu Dana (Rp)", "pagu", "#,##0", 15.0),
    ("Tanggal Mulai", "mulai", "dd/mm/yyyy", 12.0),
    ("Tanggal Selesai", "selesai", "dd/mm/yyyy", 12.0),
    ("Pekerja", "P", "0", 9.0),
    ("Hari Kerja", "Q", "0", 9.0),
    ("HOK", "R", "0", 8.0),
    ("Upah (Rp)", "S", "#,##0", 14.0),
    ("Material (Rp)", "U", "#,##0", 14.0),
    ("BOP (Rp)", "W", "#,##0", 13.0),
    ("Dana Minggu Lalu (Rp)", "Y", "#,##0", 16.0),
    ("Dana Minggu Ini (Rp)", "Z", "#,##0", 16.0),
    ("Dana Kumulatif (Rp)", "AA", "#,##0", 16.0),
    ("Keuangan Minggu Lalu", "AB", "0.00%", 11.0),
    ("Keuangan Minggu Ini", "AC", "0.00%", 11.0),
    ("Keuangan Kumulatif", "AD", "0.00%", 11.0),
    ("Fisik Minggu Lalu", "AE", "0.00%", 10.0),
    ("Fisik Minggu Ini", "AF", "0.00%", 10.0),
    ("Fisik Kumulatif", "AG", "0.00%", 10.0),
    ("Target Kurva S", "AH", "0.00%", 10.0),
    ("Deviasi", "AI", "+0.00%;-0.00%;0.00%", 10.0),
];

const RPF: &str = "[$Rp-421]#,##0;\\([$Rp-421]#,##0\\);\"-\"";
const PF: &str = "0.00%;\\(0.00%\\);\"-\"";
const AKHIR: u32 = 80;

fn serial(d: NaiveDate) -> f64 {
    (d - NaiveDate::from_ymd_opt(1899, 12, 30).unwrap()).num_days() as f64
}

/// Huruf kolom dari nomor kolom 1-based.
fn huruf(c1: u32) -> String {
    column_number_to_name((c1 - 1) as u16)
}

fn rgb(v: u32) -> Color {
    Color::RGB(v)
}

fn f_sel(fmt: Option<&str>, tebal: bool, isi: Option<u32>) -> Format {
    let mut f = Format::new().set_border(FormatBorder::Thin).set_border_color(rgb(0xD9D6CE));
    if let Some(n) = fmt {
        f = f.set_num_format(n);
    }
    if tebal {
        f = f.set_bold();
    }
    if let Some(c) = isi {
        f = f.set_background_color(rgb(c));
    }
    f
}

fn f_kepala(fill: u32) -> Format {
    Format::new()
        .set_bold()
        .set_font_color(rgb(0xFFFFFF))
        .set_background_color(rgb(fill))
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap()
        .set_border(FormatBorder::Thin)
        .set_border_color(rgb(0xD9D6CE))
}

enum V {
    N(f64),
    S(String),
    D(NaiveDate),
    B,
}

fn val(x: &Row, i: usize, key: &str) -> V {
    let teks = |s: &str| if s.is_empty() { V::B } else { V::S(s.to_string()) };
    let opt = |o: &Option<String>| match o {
        Some(s) if !s.is_empty() => V::S(s.clone()),
        _ => V::B,
    };
    match key {
        "_i" => V::N((i + 1) as f64),
        "_jenis" => V::S(x.jenis().to_string()),
        "kab" => opt(&x.kab),
        "kec" => opt(&x.kec),
        "desa" => teks(&x.desa),
        "_pokmas" => V::S(format!("Pokmas {}", x.nama)),
        "korfas" => teks(&x.korfas),
        "tahap" => opt(&x.tahap),
        "pagu" => V::N(x.pagu),
        "mulai" => x.mulai.map_or(V::B, V::D),
        "selesai" => x.selesai.map_or(V::B, V::D),
        "P" => V::N(x.p as f64),
        "Q" => V::N(x.q as f64),
        "R" => V::N(x.r as f64),
        "S" => V::N(x.s),
        "U" => V::N(x.u),
        "W" => V::N(x.w),
        "Y" => V::N(x.y),
        "Z" => V::N(x.z),
        "AA" => V::N(x.aa),
        "AB" => V::N(x.ab),
        "AC" => V::N(x.ac),
        "AD" => V::N(x.ad),
        "AE" => V::N(x.ae),
        "AF" => V::N(x.af),
        "AG" => V::N(x.ag),
        "AH" => x.ah.map_or(V::B, V::N),
        "AI" => x.ai().map_or(V::B, V::N),
        _ => V::B,
    }
}

fn tulis(ws: &mut Worksheet, r: u32, c: u32, v: &V, f: &Format) -> Result<(), XlsxError> {
    let (r0, c0) = (r - 1, (c - 1) as u16);
    match v {
        V::N(n) => ws.write_number_with_format(r0, c0, *n, f).map(|_| ()),
        V::S(s) => ws.write_string_with_format(r0, c0, s, f).map(|_| ()),
        V::D(d) => ws.write_number_with_format(r0, c0, serial(*d), f).map(|_| ()),
        V::B => ws.write_blank(r0, c0, f).map(|_| ()),
    }
}

fn teks(ws: &mut Worksheet, r: u32, c: u32, s: &str, f: &Format) -> Result<(), XlsxError> {
    ws.write_string_with_format(r - 1, (c - 1) as u16, s, f).map(|_| ())
}

fn rumus(ws: &mut Worksheet, r: u32, c: u32, s: &str, f: &Format) -> Result<(), XlsxError> {
    ws.write_formula_with_format(r - 1, (c - 1) as u16, Formula::new(s), f).map(|_| ())
}

fn kepala(ws: &mut Worksheet, baris: u32, judul: &[&str], lebar: &[f64]) -> Result<(), XlsxError> {
    let f = f_kepala(HIJAU);
    for (j, (t, w)) in judul.iter().zip(lebar).enumerate() {
        teks(ws, baris, j as u32 + 1, t, &f)?;
        ws.set_column_width(j as u16, *w)?;
    }
    ws.set_row_height(baris - 1, 32)?;
    Ok(())
}

fn th(rs: &[&Row]) -> String {
    let mut v: Vec<String> = rs.iter().filter_map(|x| x.tahap.clone()).filter(|t| !t.is_empty()).collect();
    v.sort();
    v.dedup();
    if v.len() == 1 {
        v[0].clone()
    } else if v.is_empty() {
        "Tahap 1".to_string()
    } else {
        v.join(" / ")
    }
}

/// Menulis workbook gabungan dan mengembalikan isinya.
pub fn tulis_gabungan(
    ds: &Dataset,
    lap: &Laporan,
    korfas: &str,
    tahap: Option<&str>,
    alias: &HashMap<String, String>,
    log: &[LogEntri],
) -> Result<Vec<u8>, XlsxError> {
    let minggu = lap.week;
    let rows = &lap.rows;
    let mut wb = Workbook::new();
    let mut sheets_ada: Vec<String> = Vec::new();
    let sekarang = Local::now().format("%d/%m/%Y %H:%M").to_string();
    let f_judul = Format::new().set_bold().set_font_size(13);
    let f_catat = Format::new().set_italic().set_font_color(rgb(0x6B747A));

    for k in 1..=minggu {
        let mut rs = ambil(ds, k, Some(korfas), tahap);
        if rs.is_empty() {
            continue;
        }
        for x in rs.iter_mut() {
            if let Some(b) = alias.get(&norm_nama(&x.nama)) {
                x.nama = b.clone();
            }
        }
        sheets_ada.push(format!("M{k}"));
        let ws = wb.add_worksheet();
        ws.set_name(format!("M{k}"))?;
        let judul = format!(
            "REKAPITULASI PROGRES LAPMING KEUANGAN DAN FISIK - PROV JAWA BARAT - KORFAS {}{} - MINGGU KE-{} ({})",
            korfas.to_uppercase(),
            tahap.map(|t| format!(" - {}", t.to_uppercase())).unwrap_or_default(),
            k,
            periode_sheet(&rs, k)
        );
        teks(ws, 1, 1, &judul, &f_judul)?;
        teks(
            ws,
            2,
            1,
            &format!("Digabung dari sumber Sanimas dan LPK. Sel kuning = data wajib kosong di sumber. Dibuat {sekarang}."),
            &f_catat,
        )?;
        let judul_kol: Vec<&str> = KOL_MINGGU.iter().map(|c| c.0).collect();
        let lebar: Vec<f64> = KOL_MINGGU.iter().map(|c| c.3).collect();
        kepala(ws, 4, &judul_kol, &lebar)?;
        for (i, x) in rs.iter().enumerate() {
            for (j, (_, key, fmt, _)) in KOL_MINGGU.iter().enumerate() {
                let kosong = (*key == "AH" || *key == "AI") && matches!(val(x, i, key), V::B);
                let f = f_sel(Some(fmt), false, if kosong { Some(KUNING) } else { None });
                tulis(ws, 5 + i as u32, j as u32 + 1, &val(x, i, key), &f)?;
            }
        }
        let t = 5 + rs.len() as u32;
        let n = rs.len() as f64;
        let jml = |f: &dyn Fn(&Row) -> f64| rs.iter().map(|x| f(x)).sum::<f64>();
        for (j, (_, key, fmt, _)) in KOL_MINGGU.iter().enumerate() {
            let c = j as u32 + 1;
            let f = f_sel(Some(fmt), true, Some(ABU));
            let nilai: Option<f64> = match *key {
                "pagu" => Some(jml(&|x| x.pagu)),
                "P" => Some(jml(&|x| x.p as f64)),
                "R" => Some(jml(&|x| x.r as f64)),
                "S" => Some(jml(&|x| x.s)),
                "U" => Some(jml(&|x| x.u)),
                "W" => Some(jml(&|x| x.w)),
                "Y" => Some(jml(&|x| x.y)),
                "Z" => Some(jml(&|x| x.z)),
                "AA" => Some(jml(&|x| x.aa)),
                "AB" => Some(jml(&|x| x.ab) / n),
                "AC" => Some(jml(&|x| x.ac) / n),
                "AD" => Some(jml(&|x| x.ad) / n),
                "AE" => Some(jml(&|x| x.ae) / n),
                "AF" => Some(jml(&|x| x.af) / n),
                "AG" => Some(jml(&|x| x.ag) / n),
                "AH" => {
                    let ada: Vec<f64> = rs.iter().filter_map(|x| x.ah).collect();
                    if ada.is_empty() { None } else { Some(ada.iter().sum::<f64>() / ada.len() as f64) }
                }
                "AI" => {
                    let ada: Vec<f64> = rs.iter().filter_map(|x| x.ai()).collect();
                    if ada.is_empty() { None } else { Some(ada.iter().sum::<f64>() / ada.len() as f64) }
                }
                _ => None,
            };
            if c == 1 {
                continue;
            }
            match nilai {
                Some(v) => tulis(ws, t, c, &V::N(v), &f)?,
                None => tulis(ws, t, c, &V::B, &f)?,
            }
        }
        teks(ws, t, 6, "Total / rata-rata", &f_sel(None, true, Some(ABU)))?;
        ws.set_freeze_panes(4, 6)?;
    }

    let kum = kumulatif_sheet(&mut wb, lap, tahap, &sheets_ada)?;
    anomali_sheet(&mut wb, rows, &kum)?;
    cek_data_sheet(&mut wb, lap, korfas)?;
    log_sheet(&mut wb, korfas, log)?;
    wb.save_to_buffer()
}
enum Cv {
    N(f64),
    S(String),
    F(String),
}

/// Posisi baris di sheet Kumulatif untuk dipakai sheet Anomali.
struct KumInfo {
    posisi: Vec<u32>,
    c_keu: u32,
    c_fis: u32,
}

struct Ctx<'a> {
    sheets: &'a [String],
    nc: u32,
    k_rp: usize,
    n_rp: u32,
    c_r0: u32,
    c_tot: Option<u32>,
    c_sal: u32,
    c_cek: u32,
    c_keu: u32,
    c_fis: u32,
    c_tgt: u32,
    c_dev: u32,
}

impl Ctx<'_> {
    fn fmt_angka(&self, j: u32) -> Option<&'static str> {
        match j {
            4 | 7 | 9 | 11 | 13 => Some(RPF),
            5 | 6 => Some("General"),
            8 | 10 | 12 => Some(PF),
            _ if j == self.c_sal => Some(RPF),
            _ if j == self.c_cek => Some("#,##0"),
            _ if j == self.c_keu || j == self.c_fis || j == self.c_tgt || j == self.c_dev => Some(PF),
            _ if j >= self.c_r0 && j < self.c_r0 + self.n_rp => Some(RPF),
            _ => None,
        }
    }

    fn format(&self, j: u32, tebal: bool, isi: Option<u32>) -> Format {
        let mut f = Format::new().set_font_name("Arial").set_font_size(10);
        if let Some(n) = self.fmt_angka(j) {
            f = f.set_num_format(n);
        }
        if tebal {
            f = f.set_bold();
        }
        if let Some(c) = isi {
            f = f.set_background_color(rgb(c));
        }
        f
    }

    /// Nilai kolom `key` pada sheet minggu terpilih (INDEX/MATCH per sheet, dipilih dengan CHOOSE).
    fn pilih(&self, r: u32, key: &str, kosong: bool) -> Option<String> {
        if self.sheets.is_empty() {
            return None;
        }
        let kol = kolom_minggu(key);
        let nm = kolom_minggu("_pokmas");
        let bagian: Vec<String> = self
            .sheets
            .iter()
            .map(|sh| {
                let ix = format!("INDEX('{sh}'!${kol}$5:${kol}${AKHIR},MATCH($B{r},'{sh}'!${nm}$5:${nm}${AKHIR},0))");
                if kosong {
                    format!("IF({ix}=\"\",\"\",{ix})")
                } else {
                    ix
                }
            })
            .collect();
        Some(format!("=IFERROR(CHOOSE($F$3,{}),\"\")", bagian.join(",")))
    }

    /// Jumlah kolom `key` dari sheet pertama sampai minggu terpilih, dicocokkan per nama Pokmas.
    fn jumlah(&self, r: u32, key: &str) -> Option<String> {
        if self.sheets.is_empty() {
            return None;
        }
        let kol = kolom_minggu(key);
        let nm = kolom_minggu("_pokmas");
        let bagian: Vec<String> = self
            .sheets
            .iter()
            .enumerate()
            .map(|(i, sh)| {
                format!(
                    "IF({}<=$F$3,IFERROR(SUMIF('{sh}'!${nm}$5:${nm}${AKHIR},$B{r},'{sh}'!${kol}$5:${kol}${AKHIR}),0),0)",
                    i + 1
                )
            })
            .collect();
        Some(format!("={}", bagian.join("+")))
    }
}

fn kolom_minggu(key: &str) -> String {
    let j = KOL_MINGGU.iter().position(|c| c.1 == key).unwrap_or(0) as u32 + 1;
    huruf(j)
}

fn tulis_cv(ws: &mut Worksheet, r: u32, c: u32, v: &Option<Cv>, f: &Format) -> Result<(), XlsxError> {
    match v {
        Some(Cv::N(n)) => tulis(ws, r, c, &V::N(*n), f),
        Some(Cv::S(s)) => teks(ws, r, c, s, f),
        Some(Cv::F(s)) => rumus(ws, r, c, s, f),
        None => tulis(ws, r, c, &V::B, f),
    }
}

const GRUP: u32 = 0xDDEBF7;
const ABU2: u32 = 0xF2F2F2;

fn blok(
    ws: &mut Worksheet,
    c: &Ctx,
    r0: u32,
    judul_grup: &str,
    rs: &[&Row],
    nama_total: &str,
    posisi: &mut Vec<u32>,
) -> Result<u32, XlsxError> {
    for j in 2..=c.nc {
        tulis(ws, r0, j, &V::B, &c.format(j, false, Some(GRUP)))?;
    }
    teks(ws, r0, 1, judul_grup, &c.format(1, true, Some(GRUP)))?;
    let mut r = r0 + 1;
    for (i, x) in rs.iter().enumerate() {
        let nm = format!("Pokmas {}", x.nama);
        let rt = &x.rpdb_t;
        let ada_rp = rt.iter().any(|v| v.is_some());
        let mut cells: Vec<Option<Cv>> = (0..c.nc).map(|_| None).collect();
        let mut set = |j: u32, v: Option<Cv>| cells[(j - 1) as usize] = v;
        set(1, Some(Cv::N((i + 1) as f64)));
        set(2, Some(Cv::S(nm.clone())));
        set(3, Some(Cv::F(format!("=TRIM(SUBSTITUTE(B{r},\"Pokmas \",\"\"))"))));
        set(4, c.pilih(r, "pagu", false).map(Cv::F));
        set(5, c.jumlah(r, "Q").map(Cv::F));
        set(6, c.jumlah(r, "R").map(Cv::F));
        set(7, c.jumlah(r, "S").map(Cv::F));
        set(8, Some(Cv::F(format!("=IFERROR(G{r}/$D{r},0)"))));
        set(9, c.jumlah(r, "U").map(Cv::F));
        set(10, Some(Cv::F(format!("=IFERROR(I{r}/$D{r},0)"))));
        set(11, c.jumlah(r, "W").map(Cv::F));
        set(12, Some(Cv::F(format!("=IFERROR(K{r}/$D{r},0)"))));
        set(13, Some(Cv::F(format!("=G{r}+I{r}+K{r}"))));
        for t in 0..c.k_rp {
            if let Some(Some(v)) = rt.get(t) {
                set(c.c_r0 + t as u32, Some(Cv::N(*v as f64)));
            }
        }
        if let Some(ct) = c.c_tot {
            if ada_rp {
                set(
                    ct,
                    Some(Cv::F(format!("=SUM({}{r}:{}{r})", huruf(c.c_r0), huruf(c.c_r0 + c.k_rp as u32 - 1)))),
                );
            }
        }
        let tot_ref = huruf(c.c_tot.unwrap_or(c.c_r0));
        if ada_rp {
            set(c.c_sal, Some(Cv::F(format!("={tot_ref}{r}-M{r}"))));
        }
        set(c.c_cek, Some(Cv::F(format!("=IFERROR(G{r}/F{r},0)"))));
        set(c.c_keu, c.pilih(r, "AD", false).map(Cv::F));
        set(c.c_fis, c.pilih(r, "AG", false).map(Cv::F));
        set(c.c_tgt, c.pilih(r, "AH", true).map(Cv::F));
        let (fis, tgt) = (huruf(c.c_fis), huruf(c.c_tgt));
        set(
            c.c_dev,
            Some(Cv::F(format!("=IF(OR({fis}{r}=\"\",{tgt}{r}=\"\",{tgt}{r}=0),\"\",{fis}{r}-{tgt}{r})"))),
        );
        posisi.push(r);
        for j in 1..=c.nc {
            tulis_cv(ws, r, j, &cells[(j - 1) as usize], &c.format(j, false, None))?;
        }
        r += 1;
    }
    let (a, z) = (r0 + 1, r - 1);
    let mut cells: Vec<Option<Cv>> = (0..c.nc).map(|_| None).collect();
    let mut set = |j: u32, v: Option<Cv>| cells[(j - 1) as usize] = v;
    set(2, Some(Cv::S(nama_total.to_string())));
    let mut sum_cols: Vec<u32> = vec![4, 5, 6, 7, 9, 11, 13];
    sum_cols.extend(c.c_r0..c.c_r0 + c.n_rp);
    sum_cols.push(c.c_sal);
    for j in sum_cols {
        let l = huruf(j);
        set(j, Some(Cv::F(format!("=SUM({l}{a}:{l}{z})"))));
    }
    for (j, n) in [(8u32, "G"), (10, "I"), (12, "K")] {
        set(j, Some(Cv::F(format!("=IFERROR({n}{r}/$D{r},0)"))));
    }
    set(c.c_cek, Some(Cv::F(format!("=IFERROR(G{r}/F{r},0)"))));
    for j in [c.c_keu, c.c_fis, c.c_tgt] {
        let l = huruf(j);
        set(j, Some(Cv::F(format!("=IFERROR(AVERAGE({l}{a}:{l}{z}),\"\")"))));
    }
    let (fis, tgt) = (huruf(c.c_fis), huruf(c.c_tgt));
    set(
        c.c_dev,
        Some(Cv::F(format!("=IF(OR({fis}{r}=\"\",{tgt}{r}=\"\",{tgt}{r}=0),\"\",{fis}{r}-{tgt}{r})"))),
    );
    for j in 1..=c.nc {
        tulis_cv(ws, r, j, &cells[(j - 1) as usize], &c.format(j, true, Some(ABU2)))?;
    }
    Ok(r)
}

fn kumulatif_sheet(
    wb: &mut Workbook,
    lap: &Laporan,
    tahap: Option<&str>,
    sheets_ada: &[String],
) -> Result<KumInfo, XlsxError> {
    let minggu = lap.week;
    let rows = &lap.rows;
    let ws = wb.add_worksheet();
    ws.set_name("Kumulatif")?;
    let f_n = Format::new().set_font_name("Arial").set_font_size(10);
    let f_b = f_n.clone().set_bold();

    let judul = format!(
        "REKAPITULASI KUMULATIF PROGRES LAPMING KEUANGAN DAN FISIK - PROV JAWA BARAT{}",
        tahap.map(|t| format!(" - {}", t.to_uppercase())).unwrap_or_default()
    );
    teks(ws, 1, 1, &judul, &Format::new().set_font_name("Arial").set_font_size(13).set_bold())?;
    teks(ws, 3, 1, "Pilih Minggu:", &f_b)?;
    let f_pilih = Format::new()
        .set_font_name("Arial")
        .set_font_size(10)
        .set_bold()
        .set_font_color(rgb(0x0000FF))
        .set_background_color(rgb(0xFFFF00))
        .set_align(FormatAlign::Center);
    teks(ws, 3, 3, &format!("M{minggu}"), &f_pilih)?;
    teks(ws, 3, 5, "Minggu ke-", &f_n)?;
    tulis(ws, 3, 6, &V::N(minggu as f64), &f_n)?;
    teks(ws, 3, 8, &format!("Kumulatif dari M1 s/d M{minggu}"), &f_n)?;

    let k_rp = rows
        .iter()
        .map(|x| x.rpdb_t.iter().enumerate().filter(|(_, v)| v.is_some()).map(|(i, _)| i + 1).max().unwrap_or(0))
        .max()
        .unwrap_or(0)
        .max(1);
    let mut rp_heads: Vec<String> = (1..=k_rp).map(|i| format!("RPDB {i} (Rp)")).collect();
    if k_rp > 1 {
        rp_heads.push("Total RPDB (Rp)".to_string());
    }
    let mut heads: Vec<String> = [
        "No", "Nama Pokmas", "Kunci Pencarian", "Pagu Dana (Rp)", "Kum. Hari Kerja", "Kum. HOK", "Kum. Upah (Rp)",
        "% Upah", "Kum. Material (Rp)", "% Material", "Kum. BOP (Rp)", "% BOP", "Kum. Realisasi Dana (Rp)",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    heads.extend(rp_heads.iter().cloned());
    heads.extend(
        ["Saldo (Rp)", "Cek HOK (Rp/HOK)", "Keuangan s/d Minggu Ini (%)", "Fisik s/d Minggu Ini (%)", "Target Fisik (Kurva S)", "Deviasi Fisik"]
            .iter()
            .map(|s| s.to_string()),
    );
    let c_r0 = 14u32;
    let c_tot = if k_rp > 1 { Some(14 + k_rp as u32) } else { None };
    let c_sal = 14 + rp_heads.len() as u32;
    let ctx = Ctx {
        sheets: sheets_ada,
        nc: c_sal + 5,
        k_rp,
        n_rp: rp_heads.len() as u32,
        c_r0,
        c_tot,
        c_sal,
        c_cek: c_sal + 1,
        c_keu: c_sal + 2,
        c_fis: c_sal + 3,
        c_tgt: c_sal + 4,
        c_dev: c_sal + 5,
    };
    let mut lebar: Vec<f64> = vec![5.5, 38.2, 25.5, 17.3, 11.8, 11.8, 17.3, 11.8, 17.3, 11.8, 17.3, 11.8, 17.3];
    lebar.extend(std::iter::repeat(17.3).take(rp_heads.len()));
    lebar.extend([17.3, 15.0, 14.0, 14.0, 14.0, 12.0]);
    let f_head = Format::new()
        .set_font_name("Arial")
        .set_font_size(10)
        .set_bold()
        .set_font_color(rgb(0xFFFFFF))
        .set_background_color(rgb(0x1F4E78))
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap();
    for (j, (h, w)) in heads.iter().zip(&lebar).enumerate() {
        teks(ws, 5, j as u32 + 1, h, &f_head)?;
        ws.set_column_width(j as u16, *w)?;
    }
    ws.set_row_height(4, 42)?;

    let n_s = sheets_ada.len() as u32;
    let c_w = ctx.nc + 3;
    let lw = huruf(c_w);
    let f_abu = Format::new().set_font_name("Arial").set_font_size(10).set_bold().set_font_color(rgb(0x808080));
    teks(ws, 5, c_w, "Daftar Minggu", &f_abu)?;
    teks(ws, 5, c_w + 1, "Urutan", &f_abu)?;
    for (i, sh) in sheets_ada.iter().enumerate() {
        teks(ws, 6 + i as u32, c_w, sh, &f_n)?;
        tulis(ws, 6 + i as u32, c_w + 1, &V::N((i + 1) as f64), &f_n)?;
    }
    ws.set_column_width((c_w - 1) as u16, 14.5)?;
    if n_s > 0 {
        let dv = DataValidation::new().allow_list_formula(Formula::new(format!("${lw}$6:${lw}${}", 5 + n_s)));
        ws.add_data_validation(2, 2, 2, 2, &dv)?;
        rumus(ws, 3, 6, &format!("=MATCH($C$3,${lw}$6:${lw}${},0)", 5 + n_s), &f_n)?;
        rumus(ws, 3, 8, "=\"Kumulatif dari M1 s/d \"&$C$3", &f_n)?;
    }

    let san_r: Vec<&Row> = rows.iter().filter(|x| !x.lpk).collect();
    let lpk_r: Vec<&Row> = rows.iter().filter(|x| x.lpk).collect();
    let mut posisi: Vec<u32> = Vec::new();
    let mut r = 6u32;
    if !san_r.is_empty() {
        r = blok(ws, &ctx, r, "SANIMAS", &san_r, "Total / Rata-rata SANIMAS", &mut posisi)? + 2;
    }
    if !lpk_r.is_empty() {
        r = blok(ws, &ctx, r, "LPK", &lpk_r, "Total / Rata-rata LPK", &mut posisi)? + 2;
    }
    let f_abu2 = Format::new().set_font_name("Arial").set_font_size(10).set_font_color(rgb(0x595959));
    teks(
        ws,
        r + 1,
        1,
        &format!(
            "Catatan: kolom Hari Kerja, HOK, Upah, Material, BOP dijumlah dari M1 s/d M{minggu} (dicocokkan per nama Pokmas). Keuangan/Fisik/Target diambil dari sheet minggu terpilih (pilih minggu di sel C3). Pagu dari sheet minggu terpilih; RPDB dari data RPDB di aplikasi."
        ),
        &f_abu2,
    )?;
    teks(ws, r + 3, 1, "Judul Chart:", &f_b)?;
    rumus(
        ws,
        r + 4,
        2,
        &format!("=\"Progres Keuangan dan Fisik Sanimas SPALD-S {} - Kumulatif s/d \"&$C$3", th(&san_r)),
        &f_n,
    )?;
    rumus(
        ws,
        r + 5,
        2,
        &format!("=\"Progres Keuangan dan Fisik LPK {} - Kumulatif s/d \"&$C$3", th(&lpk_r)),
        &f_n,
    )?;
    ws.set_freeze_panes(5, 3)?;
    Ok(KumInfo { posisi, c_keu: ctx.c_keu, c_fis: ctx.c_fis })
}

fn anomali_sheet(wb: &mut Workbook, rows: &[Row], kum: &KumInfo) -> Result<(), XlsxError> {
    let ws = wb.add_worksheet();
    ws.set_name("Anomali")?;
    let f_n = Format::new().set_font_name("Arial").set_font_size(10);
    let f_b = f_n.clone().set_bold();
    teks(
        ws,
        1,
        1,
        "ANOMALI PROGRES: FISIK LEBIH BESAR DARI KEUANGAN (KUMULATIF)",
        &Format::new().set_font_name("Arial").set_font_size(13).set_bold(),
    )?;
    teks(ws, 3, 1, "Pilih Minggu:", &f_n)?;
    rumus(ws, 3, 3, "=Kumulatif!C3", &f_n)?;
    teks(ws, 3, 5, "Minggu ke-", &f_n)?;
    rumus(ws, 3, 6, "=Kumulatif!F3", &f_n)?;
    teks(ws, 4, 1, "Anomali = Persentase Fisik s/d minggu ini > Persentase Keuangan s/d minggu ini", &f_n)?;
    let f_head = Format::new()
        .set_font_name("Arial")
        .set_font_size(10)
        .set_bold()
        .set_font_color(rgb(0xFFFFFF))
        .set_background_color(rgb(0x1F4E78))
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap();
    let heads = [
        "No", "Nama Pokmas", "Kunci Pencarian", "Keuangan s/d Minggu Ini (%)", "Fisik s/d Minggu Ini (%)",
        "Selisih (Fisik - Keuangan)", "Status",
    ];
    let lebar = [5.5, 38.0, 25.0, 16.0, 16.0, 16.0, 12.0];
    for (j, (h, w)) in heads.iter().zip(lebar).enumerate() {
        teks(ws, 6, j as u32 + 1, h, &f_head)?;
        ws.set_column_width(j as u16, w)?;
    }
    ws.set_row_height(5, 32)?;
    let f_pf = f_n.clone().set_num_format(PF);
    let f_grup = f_n.clone().set_background_color(rgb(GRUP));
    let mut ra = 7u32;
    let first = ra;
    let mut pi = kum.posisi.iter();
    let (k_keu, k_fis) = (huruf(kum.c_keu), huruf(kum.c_fis));
    for (g, lpk) in [("SANIMAS", false), ("LPK", true)] {
        let rs: Vec<&Row> = rows.iter().filter(|x| x.lpk == lpk).collect();
        if rs.is_empty() {
            continue;
        }
        for j in 2..=7 {
            tulis(ws, ra, j, &V::B, &f_grup)?;
        }
        teks(ws, ra, 1, g, &f_b.clone().set_background_color(rgb(GRUP)))?;
        ra += 1;
        for i in 0..rs.len() {
            let kr = pi.next().copied().unwrap_or(0);
            tulis(ws, ra, 1, &V::N((i + 1) as f64), &f_n)?;
            rumus(ws, ra, 2, &format!("=Kumulatif!B{kr}"), &f_n)?;
            rumus(ws, ra, 3, &format!("=Kumulatif!C{kr}"), &f_n)?;
            rumus(ws, ra, 4, &format!("=Kumulatif!{k_keu}{kr}"), &f_pf)?;
            rumus(ws, ra, 5, &format!("=Kumulatif!{k_fis}{kr}"), &f_pf)?;
            rumus(ws, ra, 6, &format!("=IF(OR(D{ra}=\"\",E{ra}=\"\"),\"\",E{ra}-D{ra})"), &f_pf)?;
            rumus(ws, ra, 7, &format!("=IF(OR(D{ra}=\"\",E{ra}=\"\"),\"\",IF(E{ra}>D{ra},\"ANOMALI\",\"Normal\"))"), &f_n)?;
            ra += 1;
        }
    }
    teks(ws, ra + 1, 1, "Jumlah Pokmas Anomali", &f_b)?;
    rumus(ws, ra + 1, 4, &format!("=COUNTIF(G{first}:G{ra},\"ANOMALI\")"), &f_b)?;
    ws.set_freeze_panes(6, 0)?;
    Ok(())
}

fn cek_data_sheet(wb: &mut Workbook, lap: &Laporan, korfas: &str) -> Result<(), XlsxError> {
    let ws = wb.add_worksheet();
    ws.set_name("Cek Data")?;
    teks(
        ws,
        1,
        1,
        &format!("CEK DATA DAN PERLU PERHATIAN - KORFAS {} - M{}", korfas.to_uppercase(), lap.week),
        &Format::new().set_bold().set_font_size(13),
    )?;
    kepala(ws, 3, &["No", "Jenis", "Nama Pokmas", "Kategori", "Keterangan"], &[5.0, 10.0, 32.0, 26.0, 80.0])?;
    let f = f_sel(None, false, None);
    let mut rc = 4u32;
    for (i, (idx, kategori, ket)) in lap.s.perhatian.iter().enumerate() {
        let x = &lap.rows[*idx];
        tulis(ws, rc, 1, &V::N((i + 1) as f64), &f)?;
        teks(ws, rc, 2, x.jenis(), &f)?;
        teks(ws, rc, 3, &format!("Pokmas {}", x.nama), &f)?;
        teks(ws, rc, 4, kategori, &f)?;
        teks(ws, rc, 5, ket, &f)?;
        rc += 1;
    }
    if rc == 4 {
        teks(ws, 4, 1, "Tidak ada temuan.", &Format::new())?;
    }
    ws.set_freeze_panes(3, 0)?;
    Ok(())
}

fn log_sheet(wb: &mut Workbook, korfas: &str, log: &[LogEntri]) -> Result<(), XlsxError> {
    let ws = wb.add_worksheet();
    ws.set_name("Log Perubahan")?;
    teks(
        ws,
        1,
        1,
        &format!("LOG PERUBAHAN DATA - KORFAS {} (terbaru di atas)", korfas.to_uppercase()),
        &Format::new().set_bold().set_font_size(13),
    )?;
    teks(
        ws,
        2,
        1,
        "Perubahan nilai pada sumber Sanimas dan LPK dibanding pemuatan sebelumnya; waktu = saat perubahan terdeteksi.",
        &Format::new().set_italic().set_font_color(rgb(0x6B747A)),
    )?;
    kepala(
        ws,
        4,
        &["Waktu", "Sumber", "Sheet", "Pokmas", "Korfas", "Kolom", "Nilai lama", "Nilai baru"],
        &[19.0, 10.0, 8.0, 30.0, 12.0, 16.0, 16.0, 16.0],
    )?;
    let f = f_sel(None, false, None);
    let mut r = 5u32;
    for e in log {
        for u in &e.ubah {
            let baris = u.f == "baris";
            let jenis = if e.jenis == "lpk" { "LPK" } else { "Sanimas" };
            let lama = if baris { String::new() } else { fmt_val(&u.f, &u.lama) };
            let baru = if baris {
                match &u.baru {
                    Some(super::log::Val::Tgl(s)) => s.clone(),
                    _ => String::new(),
                }
            } else {
                fmt_val(&u.f, &u.baru)
            };
            let kolom = if baris { "Baris" } else { field_nama(&u.f) };
            for (j, v) in [e.waktu.as_str(), jenis, e.sheet.as_str(), e.pokmas.as_str(), e.korfas.as_str(), kolom, lama.as_str(), baru.as_str()]
                .iter()
                .enumerate()
            {
                teks(ws, r, j as u32 + 1, v, &f)?;
            }
            r += 1;
        }
    }
    if r == 5 {
        teks(ws, 5, 1, "Belum ada perubahan tercatat.", &Format::new())?;
    }
    ws.set_freeze_panes(4, 0)?;
    Ok(())
}