use std::io::BufWriter;

use ::image::{imageops::FilterType, ColorType};
use printpdf::path::{PaintMode, WindingOrder};
use printpdf::*;

// A4 landscape (keputusan: landscape agar foto lebih besar).
pub const PAGE_W: f32 = 297.0;
pub const PAGE_H: f32 = 210.0;
const MARGIN: f32 = 10.0;
const TITLE_H: f32 = 14.0;
const HEADER_H: f32 = 10.0;
const FOOTER_H: f32 = 8.0;
// Tinggi baris catatan di bawah tabel.
const NOTE_H: f32 = 10.0;

const CELL_H: f32 = 26.0;
const GROUP_H: f32 = CELL_H * 2.0; // satu pemanfaat = baris Bilik + baris Tangki Septik

const COL_NO: f32 = 10.0;
const COL_NAMA: f32 = 50.0;
const COL_RT: f32 = 35.0;
const COL_JENIS: f32 = 25.0;
const COL_PHOTO: f32 = (PAGE_W - 2.0 * MARGIN - COL_NO - COL_NAMA - COL_RT - COL_JENIS) / 5.0;

// Jarak foto ke garis sel, supaya foto tidak menempel ke border tabel.
const PHOTO_INSET: f32 = 1.0;
const PHOTO_PX: u32 = 400; // lebar hasil resize sebelum di-encode JPEG
const PHOTO_JPEG_QUALITY: u8 = 80;
const PHOTO_DPI: f32 = 300.0;

const STAGES: [&str; 5] = ["0%", "25%", "50%", "75%", "100%"];
const BUILDINGS: [&str; 2] = ["Bilik", "Tangki Septik"];

/// Satu pemanfaat dalam tabel. `foto[bangunan][tahap]`, bangunan sesuai BUILDINGS, tahap sesuai STAGES.
#[derive(Default)]
pub struct Pemanfaat {
    pub no: u32,
    pub nama: String,
    pub rt_rw_dusun: String,
    pub nik: Option<String>,
    pub foto: [[Option<Photo>; 5]; 2],
}

/// Satu foto di sel. JPEG didecode saat digambar agar memori tidak membengkak.
pub struct Photo {
    pub jpeg: Vec<u8>,
}

#[derive(Default)]
pub struct RenderOptions {
    /// Gambar kotak kosong untuk sel tanpa foto.
    pub show_empty: bool,
    /// Catatan tambahan di bawah tabel. Kosong berarti tidak ditampilkan.
    pub catatan: String,
    /// Tahun anggaran pekerjaan, ditampilkan di judul.
    pub tahun: u16,
}

/// Membuat PDF dokumentasi dan mengembalikan isinya.
pub fn render(judul: &str, info: &str, rows: &[Pemanfaat], opts: &RenderOptions) -> Result<Vec<u8>, String> {
    let (doc, first_page, first_layer) =
        PdfDocument::new(judul, Mm(PAGE_W), Mm(PAGE_H), "Halaman 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| format!("{e:?}"))?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| format!("{e:?}"))?;

    let per_page = ((PAGE_H - 2.0 * MARGIN - TITLE_H - HEADER_H - FOOTER_H) / GROUP_H).floor() as usize;
    let chunks: Vec<&[Pemanfaat]> = if rows.is_empty() {
        vec![&[]]
    } else {
        rows.chunks(per_page).collect()
    };

    // Catatan ikut halaman tabel terakhir kalau muat; kalau tidak, dibuat halaman sendiri.
    let catatan = opts.catatan.trim();
    let table_top = PAGE_H - MARGIN - TITLE_H;
    let last_len = chunks.last().map_or(0, |c| c.len());
    let table_bottom = table_top - HEADER_H - last_len as f32 * GROUP_H;
    let note_new_page = !catatan.is_empty() && table_bottom - NOTE_H < MARGIN + FOOTER_H;
    let total = chunks.len() + usize::from(note_new_page);

    let mut layers = Vec::with_capacity(total);
    for i in 0..total {
        let (page, layer) = if i == 0 {
            (first_page, first_layer)
        } else {
            doc.add_page(Mm(PAGE_W), Mm(PAGE_H), format!("Halaman {}", i + 1))
        };
        layers.push(doc.get_page(page).get_layer(layer));
    }

    for (i, (layer, chunk)) in layers.iter().zip(chunks.iter()).enumerate() {
        draw_title(layer, &font_bold, opts.tahun);
        draw_header(layer, &font_bold, table_top);

        let mut group_top = table_top - HEADER_H;
        for row in chunk.iter() {
            draw_group(layer, &font, row, group_top, opts.show_empty);
            group_top -= GROUP_H;
        }
        draw_footer(layer, &font, info, i + 1, total);
    }

    if !catatan.is_empty() {
        if note_new_page {
            let layer = &layers[total - 1];
            draw_title(layer, &font_bold, opts.tahun);
            text_left(layer, &font, 8.0, MARGIN, table_top - 8.0, &format!("Catatan: {catatan}"));
            draw_footer(layer, &font, info, total, total);
        } else {
            text_left(
                &layers[chunks.len() - 1],
                &font,
                8.0,
                MARGIN,
                table_bottom - 6.0,
                &format!("Catatan: {catatan}"),
            );
        }
    }

    let mut buf = BufWriter::new(Vec::new());
    doc.save(&mut buf).map_err(|e| format!("{e:?}"))?;
    buf.into_inner().map_err(|e| e.to_string())
}

fn draw_title(layer: &PdfLayerReference, font: &IndirectFontRef, tahun: u16) {
    text_center(layer, font, 11.0, PAGE_H - MARGIN - 5.0, "DOKUMENTASI PELAKSANAAN KONSTRUKSI");
    text_center(
        layer,
        font,
        11.0,
        PAGE_H - MARGIN - 10.5,
        &format!("KEGIATAN SANIMAS TAHUN ANGGARAN {tahun}"),
    );
}

fn draw_header(layer: &PdfLayerReference, font: &IndirectFontRef, top: f32) {
    let y = top - HEADER_H;
    layer.set_fill_color(Color::Greyscale(Greyscale::new(0.85, None)));
    rect(layer, MARGIN, y, PAGE_W - 2.0 * MARGIN, HEADER_H, true);
    layer.set_fill_color(Color::Greyscale(Greyscale::new(0.0, None)));

    let text_y = y + HEADER_H / 2.0 - 1.2;
    let mut x = MARGIN;
    for (w, label) in [
        (COL_NO, "No."),
        (COL_NAMA, "Nama Pemanfaat"),
        (COL_RT, "RT/RW/Dusun"),
        (COL_JENIS, "Jenis Bangunan"),
    ] {
        rect(layer, x, y, w, HEADER_H, false);
        text_center_in(layer, font, 8.0, x, w, text_y, label);
        x += w;
    }
    for label in STAGES {
        rect(layer, x, y, COL_PHOTO, HEADER_H, false);
        text_center_in(layer, font, 8.0, x, COL_PHOTO, text_y, label);
        x += COL_PHOTO;
    }
}

fn draw_group(layer: &PdfLayerReference, font: &IndirectFontRef, row: &Pemanfaat, top: f32, show_empty: bool) {
    let x_nama = MARGIN + COL_NO;
    let x_rt = x_nama + COL_NAMA;
    let x_jenis = x_rt + COL_RT;
    let x_photo = x_jenis + COL_JENIS;

    // Kolom No, Nama, RT/RW/Dusun membentang dua baris (Bilik dan Tangki Septik).
    let group_y = top - GROUP_H;
    let mid_y = group_y + GROUP_H / 2.0 - 1.2;
    rect(layer, MARGIN, group_y, COL_NO, GROUP_H, false);
    text_center_in(layer, font, 8.0, MARGIN, COL_NO, mid_y, &row.no.to_string());
    rect(layer, x_nama, group_y, COL_NAMA, GROUP_H, false);
    text_left(layer, font, 8.0, x_nama + 1.5, mid_y + 1.5, &fit(&row.nama, 34));
    if let Some(nik) = &row.nik {
        text_left(layer, font, 6.5, x_nama + 1.5, mid_y - 2.5, &format!("NIK: {}", mask_nik(nik)));
    }
    rect(layer, x_rt, group_y, COL_RT, GROUP_H, false);
    text_left(layer, font, 8.0, x_rt + 1.5, mid_y, &fit(&row.rt_rw_dusun, 22));

    for (b, label) in BUILDINGS.iter().enumerate() {
        let row_y = top - (b as f32 + 1.0) * CELL_H;
        rect(layer, x_jenis, row_y, COL_JENIS, CELL_H, false);
        text_center_in(layer, font, 7.5, x_jenis, COL_JENIS, row_y + CELL_H / 2.0 - 1.0, label);

        for (s, photo) in row.foto[b].iter().enumerate() {
            let x = x_photo + s as f32 * COL_PHOTO;
            match photo {
                Some(photo) => {
                    rect(layer, x, row_y, COL_PHOTO, CELL_H, false);
                    let inner_w = COL_PHOTO - 2.0 * PHOTO_INSET;
                    let inner_h = CELL_H - 2.0 * PHOTO_INSET;
                    draw_photo(layer, &photo.jpeg, x + PHOTO_INSET, row_y + PHOTO_INSET, inner_w, inner_h);
                }
                None if show_empty => rect(layer, x, row_y, COL_PHOTO, CELL_H, false),
                None => {}
            }
        }
    }
}

fn draw_footer(layer: &PdfLayerReference, font: &IndirectFontRef, info: &str, page: usize, total: usize) {
    let y = MARGIN - 3.0;
    text_left(layer, font, 7.5, MARGIN, y, info);
    let label = format!("Halaman {page} / {total}");
    text_center_in(layer, font, 7.5, PAGE_W - MARGIN - 40.0, 40.0, y, &label);
}

/// Crop tengah ke rasio kotak, resize, encode JPEG, lalu tempel sebagai XObject DCT
/// sehingga ukuran PDF tetap kecil.
fn draw_photo(layer: &PdfLayerReference, bytes: &[u8], x: f32, y: f32, w: f32, h: f32) {
    let Ok(img) = ::image::load_from_memory(bytes) else {
        return;
    };
    // Foto tidak dipotong: diperkecil sampai muat di kotak, rasio asli dipertahankan.
    let resized = img.resize(PHOTO_PX, PHOTO_PX, FilterType::Triangle).to_rgb8();
    let (tw, th) = (resized.width(), resized.height());

    let mut jpeg = Vec::new();
    let mut encoder = ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, PHOTO_JPEG_QUALITY);
    if encoder
        .encode(resized.as_raw(), tw, th, ColorType::Rgb8)
        .is_err()
    {
        return;
    }

    let xobject = ImageXObject {
        width: Px(tw as usize),
        height: Px(th as usize),
        color_space: ColorSpace::Rgb,
        bits_per_component: ColorBits::Bit8,
        interpolate: true,
        image_data: jpeg,
        image_filter: Some(ImageFilter::DCT),
        smask: None,
        clipping_bbox: None,
    };

    // Skala "contain": foto dibesarkan atau dikecilkan sampai pas di dalam sel, lalu ditengahkan.
    let natural_w_mm = tw as f32 / PHOTO_DPI * 25.4;
    let natural_h_mm = th as f32 / PHOTO_DPI * 25.4;
    let scale = (w / natural_w_mm).min(h / natural_h_mm);
    let draw_w = natural_w_mm * scale;
    let draw_h = natural_h_mm * scale;
    Image::from(xobject).add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(Mm(x + (w - draw_w) / 2.0)),
            translate_y: Some(Mm(y + (h - draw_h) / 2.0)),
            scale_x: Some(scale),
            scale_y: Some(scale),
            dpi: Some(PHOTO_DPI),
            ..Default::default()
        },
    );
}

fn rect(layer: &PdfLayerReference, x: f32, y: f32, w: f32, h: f32, fill: bool) {
    let points = vec![
        (Point::new(Mm(x), Mm(y)), false),
        (Point::new(Mm(x + w), Mm(y)), false),
        (Point::new(Mm(x + w), Mm(y + h)), false),
        (Point::new(Mm(x), Mm(y + h)), false),
    ];
    let mode = if fill { PaintMode::FillStroke } else { PaintMode::Stroke };
    layer.add_polygon(Polygon {
        rings: vec![points],
        mode,
        winding_order: WindingOrder::NonZero,
    });
}

fn text_left(layer: &PdfLayerReference, font: &IndirectFontRef, size: f32, x: f32, y: f32, s: &str) {
    layer.use_text(s, size, Mm(x), Mm(y), font);
}

fn text_center(layer: &PdfLayerReference, font: &IndirectFontRef, size: f32, y: f32, s: &str) {
    let x = (PAGE_W - approx_width(s, size)) / 2.0;
    layer.use_text(s, size, Mm(x), Mm(y), font);
}

fn text_center_in(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    size: f32,
    x: f32,
    w: f32,
    y: f32,
    s: &str,
) {
    let tx = x + (w - approx_width(s, size)) / 2.0;
    layer.use_text(s, size, Mm(tx.max(x)), Mm(y), font);
}

/// Perkiraan lebar teks Helvetica dalam mm (rata-rata 0.5 em per karakter).
fn approx_width(s: &str, size_pt: f32) -> f32 {
    s.chars().count() as f32 * size_pt * 0.5 * 0.3528
}

/// NIK di PDF disamarkan: 10 digit pertama terlihat, sisanya diganti ***.
fn mask_nik(nik: &str) -> String {
    let shown: String = nik.chars().take(10).collect();
    format!("{shown}***")
}

fn fit(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// ---- Format LPK ----

/// Header dokumen LPK: identitas pokmas dan lokasi.
pub struct LpkHeader {
    pub nama_pokmas: String,
    pub nama_lpk: String,
    pub desa: String,
    pub kecamatan: String,
    pub kabupaten: String,
    pub provinsi: String,
}

const LPK_BANGUNAN: [&str; 2] = ["Bangunan Atas", "Bangunan Bawah"];
const LPK_COL_NO: f32 = 12.0;
const LPK_COL_JENIS: f32 = 35.0;
const LPK_COL_FOTO: f32 = (PAGE_W - 2.0 * MARGIN - LPK_COL_NO - LPK_COL_JENIS) / 5.0;
const LPK_HEAD_TOP_H: f32 = 6.0;
const LPK_HEAD_STAGE_H: f32 = 8.0;
const LPK_CELL_H: f32 = 60.0;
const LPK_LABEL_W: f32 = 32.0;

/// Membuat PDF format LPK: satu halaman per pokmas dengan baris Bangunan Atas dan Bawah.
/// `foto[bangunan][tahap]` mengikuti urutan LPK_BANGUNAN dan STAGES.
pub fn render_lpk(header: &LpkHeader, foto: &[[Option<Photo>; 5]; 2]) -> Result<Vec<u8>, String> {
    let (doc, page, layer_id) = PdfDocument::new(
        "Dokumentasi Pelaksanaan Konstruksi Sanitasi LPK",
        Mm(PAGE_W),
        Mm(PAGE_H),
        "Halaman 1",
    );
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| format!("{e:?}"))?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| format!("{e:?}"))?;
    let layer = doc.get_page(page).get_layer(layer_id);

    text_center(
        &layer,
        &font_bold,
        11.0,
        PAGE_H - MARGIN - 5.0,
        "DOKUMENTASI PELAKSANAAN KONSTRUKSI KEGIATAN SANITASI LPK",
    );

    let kiri = [
        ("Nama Pokmas", header.nama_pokmas.as_str()),
        ("Nama LPK", header.nama_lpk.as_str()),
        ("Desa/Kelurahan", header.desa.as_str()),
    ];
    let kanan = [
        ("Kecamatan", header.kecamatan.as_str()),
        ("Kab/Kota", header.kabupaten.as_str()),
        ("Provinsi", header.provinsi.as_str()),
    ];
    let info_top = PAGE_H - MARGIN - 18.0;
    let x_kanan = PAGE_W / 2.0 + 10.0;
    for (i, (label, value)) in kiri.iter().enumerate() {
        identitas(&layer, &font, MARGIN, info_top - i as f32 * 8.0, label, value);
    }
    for (i, (label, value)) in kanan.iter().enumerate() {
        identitas(&layer, &font, x_kanan, info_top - i as f32 * 8.0, label, value);
    }

    let table_top = info_top - 3.0 * 8.0 - 4.0;
    let head_h = LPK_HEAD_TOP_H + LPK_HEAD_STAGE_H;
    let head_y = table_top - head_h;
    let x_jenis = MARGIN + LPK_COL_NO;
    let x_foto = x_jenis + LPK_COL_JENIS;
    let foto_w = LPK_COL_FOTO * 5.0;

    // Header: No dan Jenis membentang dua baris, "Dokumentasi" di atas kolom tahap.
    layer.set_fill_color(Color::Greyscale(Greyscale::new(0.85, None)));
    rect(&layer, MARGIN, head_y, PAGE_W - 2.0 * MARGIN, head_h, true);
    layer.set_fill_color(Color::Greyscale(Greyscale::new(0.0, None)));
    rect(&layer, MARGIN, head_y, LPK_COL_NO, head_h, false);
    text_center_in(&layer, &font, 8.0, MARGIN, LPK_COL_NO, head_y + head_h / 2.0 - 1.2, "No.");
    rect(&layer, x_jenis, head_y, LPK_COL_JENIS, head_h, false);
    text_center_in(&layer, &font, 8.0, x_jenis, LPK_COL_JENIS, head_y + head_h / 2.0 - 1.2, "Jenis Bangunan");
    rect(&layer, x_foto, head_y + LPK_HEAD_STAGE_H, foto_w, LPK_HEAD_TOP_H, false);
    text_center_in(&layer, &font, 8.0, x_foto, foto_w, head_y + LPK_HEAD_STAGE_H + 1.8, "Dokumentasi");
    for (s, label) in STAGES.iter().enumerate() {
        let x = x_foto + s as f32 * LPK_COL_FOTO;
        rect(&layer, x, head_y, LPK_COL_FOTO, LPK_HEAD_STAGE_H, false);
        text_center_in(&layer, &font, 8.0, x, LPK_COL_FOTO, head_y + 2.6, label);
    }

    for (b, label) in LPK_BANGUNAN.iter().enumerate() {
        let row_y = head_y - (b as f32 + 1.0) * LPK_CELL_H;
        let mid_y = row_y + LPK_CELL_H / 2.0 - 1.2;
        rect(&layer, MARGIN, row_y, LPK_COL_NO, LPK_CELL_H, false);
        text_center_in(&layer, &font, 8.0, MARGIN, LPK_COL_NO, mid_y, &(b + 1).to_string());
        rect(&layer, x_jenis, row_y, LPK_COL_JENIS, LPK_CELL_H, false);
        text_center_in(&layer, &font, 8.0, x_jenis, LPK_COL_JENIS, mid_y, label);
        for (s, photo) in foto[b].iter().enumerate() {
            let x = x_foto + s as f32 * LPK_COL_FOTO;
            rect(&layer, x, row_y, LPK_COL_FOTO, LPK_CELL_H, false);
            if let Some(photo) = photo {
                let inner_w = LPK_COL_FOTO - 2.0 * PHOTO_INSET;
                let inner_h = LPK_CELL_H - 2.0 * PHOTO_INSET;
                draw_photo(&layer, &photo.jpeg, x + PHOTO_INSET, row_y + PHOTO_INSET, inner_w, inner_h);
            }
        }
    }

    let mut buf = BufWriter::new(Vec::new());
    doc.save(&mut buf).map_err(|e| format!("{e:?}"))?;
    buf.into_inner().map_err(|e| e.to_string())
}

fn identitas(layer: &PdfLayerReference, font: &IndirectFontRef, x: f32, y: f32, label: &str, value: &str) {
    text_left(layer, font, 9.0, x, y, label);
    text_left(layer, font, 9.0, x + LPK_LABEL_W, y, ":");
    text_left(layer, font, 9.0, x + LPK_LABEL_W + 3.0, y, &fit(value, 60));
}
