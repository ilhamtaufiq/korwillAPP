//! Checklist berkas per pokmas. Daftar pokmas mengikuti korwil akun yang login; admin memilih korwil lewat query.

use std::collections::BTreeMap;
use std::io::Cursor;

use axum::{
    body::Bytes,
    extract::{Query, State},
    Json,
};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_sessions::Session;

use crate::{
    auth::current_user,
    berkas::BERKAS,
    error::AppError,
    laporan_mingguan::api::korwil_izin,
    lapming::{api as lap_api, fmt::norm_nama},
    AppState,
};

const SHEET: &str = "LEMBAR KENDALI";
const BARIS_DATA_AWAL: u32 = 7;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Info {
    pub no: String,
    pub tahap: String,
    pub kab: String,
    pub kec: String,
    pub desa: String,
    pub nama: String,
    pub tfl: String,
    pub korfas: String,
}

#[derive(Clone, Debug)]
pub struct Impor {
    pub kunci: String,
    pub info: Info,
    pub centang: BTreeMap<String, String>,
}

fn indeks_kolom(huruf: &str) -> u32 {
    huruf.bytes().fold(0, |n, b| n * 26 + (b - b'A' + 1) as u32)
}

fn tanggal_dari_serial(serial: f64) -> Option<String> {
    let d = chrono::NaiveDate::from_ymd_opt(1899, 12, 30)? + chrono::Duration::days(serial.floor() as i64);
    Some(d.format("%Y-%m-%d").to_string())
}

/// Teks sel. Angka bulat ditulis tanpa ".0", tanggal sebagai YYYY-MM-DD, sel kosong sebagai "".
fn teks(v: &Data) -> String {
    match v {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) if f.fract() == 0.0 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::DateTime(dt) => tanggal_dari_serial(dt.as_f64()).unwrap_or_default(),
        Data::DateTimeIso(s) => s.get(..10).unwrap_or("").to_string(),
        Data::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// Nilai ceklis: "Bukti" (huruf berapa pun) disamakan; selain itu nilai apa adanya.
fn nilai_centang(s: String) -> String {
    if s.eq_ignore_ascii_case("bukti") {
        "Bukti".to_string()
    } else {
        s
    }
}

/// Membaca Lembar Kendali. Baris pokmas = baris yang punya nama di kolom G (mulai baris 7).
pub fn baca_lembar(bytes: &[u8]) -> Result<Vec<Impor>, String> {
    let mut wb: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes.to_vec())).map_err(|e| format!("File bukan xlsx yang bisa dibaca: {e}"))?;
    let r = wb
        .worksheet_range(SHEET)
        .map_err(|_| format!("Sheet '{SHEET}' tidak ditemukan. Pastikan file adalah Lembar Kendali POKMAS."))?;
    let (r0, c0) = r.start().unwrap_or((0, 0));
    let ambil = |baris: u32, kolom: &str| -> String {
        let c = indeks_kolom(kolom);
        match r.get(((baris - 1).saturating_sub(r0) as usize, (c - 1).saturating_sub(c0) as usize)) {
            Some(v) => teks(v),
            None => String::new(),
        }
    };
    let akhir = r.end().map(|(b, _)| b + 1).unwrap_or(0);
    let mut out = Vec::new();
    for baris in BARIS_DATA_AWAL..=akhir {
        let nama = ambil(baris, "G");
        if nama.is_empty() {
            continue;
        }
        let info = Info {
            no: ambil(baris, "B"),
            tahap: ambil(baris, "C"),
            kab: ambil(baris, "D"),
            kec: ambil(baris, "E"),
            desa: ambil(baris, "F"),
            nama: nama.clone(),
            tfl: ambil(baris, "H"),
            korfas: ambil(baris, "I"),
        };
        let kunci = format!(
            "{}|{}|{}|{}",
            norm_nama(&info.kab),
            norm_nama(&info.kec),
            norm_nama(&info.desa),
            norm_nama(&info.nama)
        );
        let mut centang = BTreeMap::new();
        for (kode, _, _) in BERKAS {
            let v = nilai_centang(ambil(baris, kode));
            if !v.is_empty() {
                centang.insert(kode.to_string(), v);
            }
        }
        out.push(Impor { kunci, info, centang });
    }
    Ok(out)
}

/// Pokmas korwil yang login (admin: korwil dari query).
pub async fn daftar(State(st): State<AppState>, session: Session, Query(q): Query<KorwilQuery>) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    lap_api::butuh_lapming(&user)?;
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, q.korwil.as_deref()).await?;
    let baris: Vec<(String, String, String)> =
        sqlx::query_as("SELECT kunci, info, centang FROM checklist_pokmas WHERE korwil = ? ORDER BY kunci")
            .bind(&korwil)
            .fetch_all(&st.db)
            .await?;
    let mut pokmas = Vec::new();
    for (kunci, info, centang) in baris {
        let info: Info = serde_json::from_str(&info).unwrap_or_default();
        let centang: BTreeMap<String, String> = serde_json::from_str(&centang).unwrap_or_default();
        pokmas.push(json!({ "kunci": kunci, "info": info, "centang": centang }));
    }
    Ok(Json(json!({
        "korwil": korwil,
        "items": BERKAS.iter().map(|(k, g, l)| json!({ "kode": k, "grup": g, "label": l })).collect::<Vec<_>>(),
        "pokmas": pokmas,
    })))
}

#[derive(Deserialize)]
pub struct KorwilQuery {
    korwil: Option<String>,
}

#[derive(Deserialize)]
pub struct CentangIn {
    korwil: Option<String>,
    kunci: String,
    kode: String,
    /// Kosong = hapus ceklis.
    nilai: String,
}

/// Mengubah satu ceklis. Admin wajib mengirim korwil.
pub async fn centang(State(st): State<AppState>, session: Session, Json(body): Json<CentangIn>) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    lap_api::butuh_lapming(&user)?;
    if !BERKAS.iter().any(|(k, _, _)| *k == body.kode) {
        return Err(AppError::BadRequest(format!("Kode berkas tidak dikenal: {}", body.kode)));
    }
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, body.korwil.as_deref()).await?;
    let lama: Option<String> = sqlx::query_scalar("SELECT centang FROM checklist_pokmas WHERE korwil = ? AND kunci = ?")
        .bind(&korwil)
        .bind(&body.kunci)
        .fetch_optional(&st.db)
        .await?;
    let Some(lama) = lama else {
        return Err(AppError::NotFound);
    };
    let mut centang: BTreeMap<String, String> = serde_json::from_str(&lama).unwrap_or_default();
    let nilai = body.nilai.trim();
    if nilai.is_empty() {
        centang.remove(&body.kode);
    } else {
        centang.insert(body.kode.clone(), nilai_centang(nilai.to_string()));
    }
    sqlx::query("UPDATE checklist_pokmas SET centang = ? WHERE korwil = ? AND kunci = ?")
        .bind(serde_json::to_string(&centang).map_err(|e| AppError::BadRequest(e.to_string()))?)
        .bind(&korwil)
        .bind(&body.kunci)
        .execute(&st.db)
        .await?;
    Ok(Json(json!({ "ok": true, "centang": centang })))
}

/// Impor Lembar Kendali (xlsx) untuk korwil yang dipilih. Data pokmas korwil itu diganti seluruhnya.
pub async fn impor(State(st): State<AppState>, session: Session, Query(q): Query<KorwilQuery>, body: Bytes) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    lap_api::butuh_lapming(&user)?;
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, q.korwil.as_deref()).await?;
    let hasil = baca_lembar(&body).map_err(AppError::BadRequest)?;
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM checklist_pokmas WHERE korwil = ?").bind(&korwil).execute(&mut *tx).await?;
    for p in &hasil {
        sqlx::query("INSERT INTO checklist_pokmas (korwil, kunci, info, centang) VALUES (?, ?, ?, ?)")
            .bind(&korwil)
            .bind(&p.kunci)
            .bind(serde_json::to_string(&p.info).map_err(|e| AppError::BadRequest(e.to_string()))?)
            .bind(serde_json::to_string(&p.centang).map_err(|e| AppError::BadRequest(e.to_string()))?)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "pokmas": hasil.len() })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impor_membaca_lembar_kendali_contoh() {
        let path = "/root/.claude/uploads/ed2718b6-4e6a-595b-9618-806d016befd8/d958fa33-01._Lembar_Kendali_POKMAS_SPALD-S_2026.xlsx";
        let Ok(bytes) = std::fs::read(path) else { return };
        let hasil = baca_lembar(&bytes).unwrap();
        assert_eq!(hasil.len(), 64);
        let maju = hasil.iter().find(|p| p.info.nama.starts_with("MAJU JAYA")).expect("MAJU JAYA ada");
        assert_eq!(maju.info.kab, "Bekasi");
        assert_eq!(maju.centang.get("AD").map(String::as_str), Some("Bukti"));
    }
}
