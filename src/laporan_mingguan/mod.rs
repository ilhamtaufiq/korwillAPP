//! Form Laporan Mingguan Korwil: template resmi (Tahapan, Target, Fisik & HOK, Keuangan, KURVA-S, Permasalahan)
//! diisi dari data Lapming. Yang belum punya sumber data (tanggal tahapan, target, partisipan, permasalahan,
//! TPS3R, KURVA-S) dibiarkan kosong, sesuai rencana bertahap.

mod isi;

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{
    auth::current_user,
    error::{internal, AppError},
    lapming::{api as lap_api, calc, fmt::norm_nama, model::Row},
    AppState,
};

use isi::{Isian, Nilai};

const TEMPLATE: &[u8] = include_bytes!("../../templates/laporan_mingguan.xlsx");

/// Kapasitas baris form. Sanimas dibatasi Tahapan/Target/Fisik (50); LPK dibatasi Target/Fisik (45).
const KAPASITAS_SAN: usize = 50;
const KAPASITAS_LPK: usize = 45;

fn teks(s: &str) -> Nilai {
    Nilai::Teks(s.to_string())
}

fn angka<T: Into<f64>>(v: T) -> Nilai {
    Nilai::Angka(v.into())
}

/// Ribuan rupiah, sesuai satuan Rp.000 di form.
fn ribu(rp: f64) -> Nilai {
    Nilai::Angka(rp / 1000.0)
}

fn isian(sheet: &str, sel: String, nilai: Nilai) -> Isian {
    (sheet.to_string(), sel, nilai)
}

/// Mengubah hasil perhitungan Lapming menjadi daftar isian sel untuk template.
pub fn susun_isian(rows: &[Row], korwil: &str, periode: &str) -> Result<Vec<Isian>, String> {
    let san: Vec<&Row> = rows.iter().filter(|x| !x.lpk).collect();
    let lpk: Vec<&Row> = rows.iter().filter(|x| x.lpk).collect();
    if san.len() > KAPASITAS_SAN {
        return Err(format!("Jumlah lokasi Sanimas {} melebihi kapasitas form ({KAPASITAS_SAN})", san.len()));
    }
    if lpk.len() > KAPASITAS_LPK {
        return Err(format!("Jumlah lokasi LPK {} melebihi kapasitas form ({KAPASITAS_LPK})", lpk.len()));
    }

    let mut out: Vec<Isian> = Vec::new();
    // Header di Tahapan Sanimas; sheet lain mengambilnya lewat rumus.
    out.push(isian("Tahapan Sanimas", "C5".into(), teks(korwil)));
    out.push(isian("Tahapan Sanimas", "C6".into(), teks(periode)));
    out.push(isian("Tahapan Sanimas", "E6".into(), teks("")));

    // Sanimas: Tahapan baris 12+i, Fisik & HOK baris 13+i.
    for (i, x) in san.iter().enumerate() {
        let t = 12 + i;
        let f = 13 + i;
        let s = &x.b;
        out.push(isian("Tahapan Sanimas", format!("A{t}"), angka((i + 1) as f64)));
        out.push(isian("Tahapan Sanimas", format!("B{t}"), teks("Jawa Barat")));
        out.push(isian("Tahapan Sanimas", format!("C{t}"), teks(x.kab.as_deref().unwrap_or(""))));
        out.push(isian("Tahapan Sanimas", format!("D{t}"), teks(x.kec.as_deref().unwrap_or(""))));
        out.push(isian("Tahapan Sanimas", format!("E{t}"), teks(&x.desa)));
        out.push(isian("Tahapan Sanimas", format!("F{t}"), teks(x.tahap.as_deref().unwrap_or(""))));

        out.push(isian("Fisik & HOK Sanimas", format!("G{f}"), ribu(x.pagu)));
        out.push(isian("Fisik & HOK Sanimas", format!("T{f}"), angka(x.p as f64)));
        out.push(isian("Fisik & HOK Sanimas", format!("W{f}"), angka(s.q)));
        out.push(isian("Fisik & HOK Sanimas", format!("X{f}"), angka(x.r as f64)));
        out.push(isian("Fisik & HOK Sanimas", format!("Y{f}"), angka(s.r)));
        out.push(isian("Fisik & HOK Sanimas", format!("Z{f}"), ribu(s.s - x.s)));
        out.push(isian("Fisik & HOK Sanimas", format!("AA{f}"), ribu(x.s)));
        if let Some(target) = x.ah {
            out.push(isian("Fisik & HOK Sanimas", format!("Q{f}"), angka(target)));
        }
        out.push(isian("Fisik & HOK Sanimas", format!("R{f}"), angka(x.ag)));
    }

    // LPK: Tahapan baris 11+i, Fisik & HOK LPK baris 13+i.
    for (i, x) in lpk.iter().enumerate() {
        let t = 11 + i;
        let f = 13 + i;
        let s = &x.b;
        out.push(isian("Tahapan LPK", format!("A{t}"), angka((i + 1) as f64)));
        out.push(isian("Tahapan LPK", format!("B{t}"), teks("Jawa Barat")));
        out.push(isian("Tahapan LPK", format!("C{t}"), teks(x.kab.as_deref().unwrap_or(""))));
        out.push(isian("Tahapan LPK", format!("D{t}"), teks(x.kec.as_deref().unwrap_or(""))));
        out.push(isian("Tahapan LPK", format!("E{t}"), teks(&x.desa)));
        out.push(isian("Tahapan LPK", format!("F{t}"), teks(&x.nama)));
        out.push(isian("Tahapan LPK", format!("G{t}"), teks(x.tahap.as_deref().unwrap_or(""))));

        out.push(isian("Fisik & HOK LPK", format!("H{f}"), ribu(x.pagu)));
        out.push(isian("Fisik & HOK LPK", format!("U{f}"), angka(x.p as f64)));
        out.push(isian("Fisik & HOK LPK", format!("X{f}"), angka(s.q)));
        out.push(isian("Fisik & HOK LPK", format!("Y{f}"), angka(x.r as f64)));
        out.push(isian("Fisik & HOK LPK", format!("Z{f}"), angka(s.r)));
        out.push(isian("Fisik & HOK LPK", format!("AA{f}"), ribu(s.s - x.s)));
        out.push(isian("Fisik & HOK LPK", format!("AB{f}"), ribu(x.s)));
        if let Some(target) = x.ah {
            out.push(isian("Fisik & HOK LPK", format!("R{f}"), angka(target)));
        }
        out.push(isian("Fisik & HOK LPK", format!("S{f}"), angka(x.ag)));
    }
    Ok(out)
}

/// Membuat file xlsx dari template untuk satu korwil dan satu minggu.
pub fn bangun(rows: &[Row], minggu: u32, korwil: &str) -> Result<Vec<u8>, String> {
    let periode = calc::periode_sheet(rows, minggu);
    let isian = susun_isian(rows, korwil, &periode)?;
    isi::isi_template(TEMPLATE, &isian)
}

#[derive(Deserialize)]
pub struct FormQuery {
    korwil: Option<String>,
    minggu: Option<String>,
}

/// Korwil dipilih dari query, kecuali akun korwil yang selalu memakai korwilnya sendiri.
async fn korwil_terpilih(st: &AppState, user: &crate::auth::User, ds: &crate::lapming::model::Dataset, q: Option<&str>) -> Result<String, AppError> {
    let daftar = calc::daftar_korfas(ds);
    if user.role == "korwil" {
        let nama: Option<String> = sqlx::query_scalar::<_, Option<String>>("SELECT nama FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_optional(&st.db)
            .await?
            .flatten();
        let kunci = nama.as_deref().map(norm_nama).unwrap_or_default();
        return daftar
            .into_iter()
            .find(|k| !kunci.is_empty() && norm_nama(k) == kunci)
            .ok_or_else(|| AppError::BadRequest("Nama akun belum cocok dengan korwil di data Lapming.".into()));
    }
    let pilih = q.map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| AppError::BadRequest("Pilih korwil dulu.".into()))?;
    daftar
        .into_iter()
        .find(|k| k == pilih)
        .ok_or_else(|| AppError::BadRequest(format!("Korwil '{pilih}' tidak ada di data Lapming.")))
}

/// Unduh form laporan mingguan (.xlsx) untuk satu korwil dan minggu.
pub async fn unduh(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<FormQuery>,
) -> Result<Response, AppError> {
    let user = current_user(&session, &st.db).await?;
    lap_api::butuh_lapming(&user)?;
    let ds = lap_api::muat_dataset(&st.db).await?;
    let pokmas = lap_api::muat_pokmas(&st.db).await?;
    let korwil = korwil_terpilih(&st, &user, &ds, q.korwil.as_deref()).await?;
    let pilih: Option<u32> = q
        .minggu
        .as_deref()
        .map(|m| m.trim().trim_start_matches(['M', 'm']))
        .filter(|m| !m.is_empty())
        .and_then(|m| m.parse().ok());

    let (isi, minggu) = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, u32), String> {
        let lap = calc::susun(&ds, &korwil, pilih, None, &pokmas)?;
        let isi = bangun(&lap.rows, lap.week, &korwil)?;
        Ok((isi, lap.week))
    })
    .await
    .map_err(internal)?
    .map_err(AppError::BadRequest)?;

    let nama = q.korwil.clone().unwrap_or_default();
    let nama_berkas: String = nama.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"Form Laporan Mingguan - {} - M{minggu}.xlsx\"", nama_berkas.trim()),
            ),
        ],
        isi,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
    use std::io::{Cursor, Read};

    fn baris(lpk: bool, nama: &str, desa: &str, pagu: f64) -> Row {
        let mut r = Row::default();
        r.lpk = lpk;
        r.nama = nama.into();
        r.desa = desa.into();
        r.kab = Some("Bogor".into());
        r.kec = Some("Ciomas".into());
        r.tahap = Some("Tahap 1".into());
        r.pagu = pagu;
        r.p = 12;
        r.r = 72;
        r.s = 8485.0;
        r.ag = 0.6604;
        r.ah = Some(0.8468);
        r.b.q = 744.0;
        r.b.r = 1000.0;
        r.b.s = 94725.0;
        r
    }

    fn buka(bytes: &[u8]) -> calamine::Xlsx<Cursor<Vec<u8>>> {
        open_workbook_from_rs(Cursor::new(bytes.to_vec())).expect("xlsx terbaca")
    }

    fn nilai(wb: &mut calamine::Xlsx<Cursor<Vec<u8>>>, sheet: &str, sel: &str) -> String {
        let r = wb.worksheet_range(sheet).expect("sheet ada");
        let (row, col) = {
            let re = regex::Regex::new(r"^([A-Z]+)(\d+)$").unwrap();
            let c = re.captures(sel).unwrap();
            let mut n = 0u32;
            for ch in c[1].chars() {
                n = n * 26 + (ch as u32 - 'A' as u32 + 1);
            }
            (c[2].parse::<u32>().unwrap() - 1, n - 1)
        };
        match r.get((row as usize, col as usize)) {
            Some(Data::String(s)) => s.clone(),
            Some(Data::Float(f)) => format!("{f}"),
            Some(Data::Int(i)) => format!("{i}"),
            Some(Data::Empty) => String::new(),
            Some(other) => format!("{other:?}"),
            None => String::new(),
        }
    }

    #[test]
    fn template_sudah_bersih_dan_terbaca() {
        let mut wb = buka(TEMPLATE);
        assert_eq!(nilai(&mut wb, "Tahapan Sanimas", "A9"), "No.");
        assert_eq!(nilai(&mut wb, "Partisipan Masyarakat", "I7"), "PIC Korwil");
        // Data contoh (Bogor, Pagelaran) sudah hilang dari baris data.
        assert_eq!(nilai(&mut wb, "Tahapan Sanimas", "E12"), "");
    }

    #[test]
    fn isi_mengganti_nilai_dan_menjaga_rumus() {
        let isian = vec![("Tahapan Sanimas".to_string(), "C5".to_string(), Nilai::Teks("Ganthi".into()))];
        let out = isi::isi_template(TEMPLATE, &isian).unwrap();
        let mut wb = buka(&out);
        assert_eq!(nilai(&mut wb, "Tahapan Sanimas", "C5"), "Ganthi");
        // Rumus di sheet lain tetap ada (tidak ikut tertimpa).
        let mut xml = String::new();
        zip::ZipArchive::new(Cursor::new(out.clone()))
            .unwrap()
            .by_name("xl/worksheets/sheet8.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("<f>") && xml.contains("Tahapan Sanimas"));
    }

    #[test]
    fn sel_tidak_ada_ditolak() {
        let isian = vec![("Tahapan Sanimas".to_string(), "ZZ9999".to_string(), Nilai::Angka(1.0))];
        let err = isi::isi_template(TEMPLATE, &isian).unwrap_err();
        assert!(err.contains("ZZ9999"), "{err}");
    }

    #[test]
    fn lokasi_sanimas_dan_lpk_masuk_ke_baris_yang_benar() {
        let rows = vec![
            baris(false, "", "Pagelaran", 400_000_000.0),
            baris(false, "", "Cimandala", 300_000_000.0),
            baris(true, "PP Daarul", "Tanjung Rasa", 250_000_000.0),
        ];
        let out = bangun(&rows, 9, "Ganthi").unwrap();
        let mut wb = buka(&out);
        // Sanimas: Tahapan baris 12+i, Fisik baris 13+i.
        assert_eq!(nilai(&mut wb, "Tahapan Sanimas", "E12"), "Pagelaran");
        assert_eq!(nilai(&mut wb, "Tahapan Sanimas", "E13"), "Cimandala");
        assert_eq!(nilai(&mut wb, "Fisik & HOK Sanimas", "G13"), "400000");
        assert_eq!(nilai(&mut wb, "Fisik & HOK Sanimas", "T13"), "12");
        assert_eq!(nilai(&mut wb, "Fisik & HOK Sanimas", "Q13"), "0.8468");
        assert_eq!(nilai(&mut wb, "Fisik & HOK Sanimas", "R13"), "0.6604");
        // Upah kumulatif minggu lalu = kumulatif - minggu ini (Rp.000).
        assert_eq!(nilai(&mut wb, "Fisik & HOK Sanimas", "Z13"), "86.24");
        assert_eq!(nilai(&mut wb, "Fisik & HOK Sanimas", "AA13"), "8.485");
        // LPK: Tahapan baris 11+i, Fisik baris 13+i, dengan nama LPK di kolom F.
        assert_eq!(nilai(&mut wb, "Tahapan LPK", "F11"), "PP Daarul");
        assert_eq!(nilai(&mut wb, "Fisik & HOK LPK", "H13"), "250000");
        assert_eq!(nilai(&mut wb, "Fisik & HOK LPK", "U13"), "12");
        // Header periode dan korwil.
        assert_eq!(nilai(&mut wb, "Tahapan Sanimas", "C5"), "Ganthi");
    }

    #[test]
    fn melebihi_kapasitas_ditolak() {
        let rows: Vec<Row> = (0..51).map(|i| baris(false, "", &format!("Desa {i}"), 1.0)).collect();
        assert!(bangun(&rows, 9, "Ganthi").unwrap_err().contains("melebihi kapasitas"));
    }

    /// Menulis contoh file untuk diperiksa di LibreOffice: `cargo test tulis_contoh -- --ignored`.
    #[test]
    #[ignore]
    fn tulis_contoh() {
        let rows = vec![
            baris(false, "", "Pagelaran", 400_000_000.0),
            baris(false, "", "Cimandala", 300_000_000.0),
            baris(true, "PP Daarul", "Tanjung Rasa", 250_000_000.0),
        ];
        let out = bangun(&rows, 9, "Ganthi").unwrap();
        let tujuan = std::env::var("CONTOH_OUT").unwrap_or_else(|_| "/tmp/contoh-form.xlsx".into());
        std::fs::write(tujuan, out).unwrap();
    }
}
