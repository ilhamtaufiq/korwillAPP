//! Endpoint isian Form Laporan Mingguan: daftar per korwil, simpan lokasi, simpan baris, dan impor.

use std::collections::{BTreeMap, BTreeSet};

use axum::{
    body::Bytes,
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_sessions::Session;

use super::{impor, store};
use crate::{
    auth::{current_user, User},
    error::AppError,
    lapming::{api as lap_api, calc, fmt::norm_nama, model::Dataset},
    AppState,
};

#[derive(Deserialize)]
pub struct KorwilQuery {
    korwil: Option<String>,
}

/// Korwil yang boleh diakses: akun korwil hanya korwilnya sendiri; admin memilih dari query/body.
pub(crate) async fn korwil_izin(st: &AppState, user: &User, ds: &Dataset, pilih: Option<&str>) -> Result<String, AppError> {
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
    let pilih = pilih.map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| AppError::BadRequest("Pilih korwil dulu.".into()))?;
    daftar
        .into_iter()
        .find(|k| k == pilih)
        .ok_or_else(|| AppError::BadRequest(format!("Korwil '{pilih}' tidak ada di data Lapming.")))
}

fn kolom_json(daftar: &[(&str, &str)]) -> Value {
    Value::Array(daftar.iter().map(|(k, v)| json!([k, v])).collect())
}

/// Lokasi yang muncul di data Lapming untuk satu korwil (semua minggu), tanpa duplikat.
fn lokasi_lapming(ds: &Dataset, korwil: &str) -> Vec<Value> {
    let kunci_korwil = norm_nama(korwil);
    let mut sudah: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out = Vec::new();
    for (lpk, b) in ds.books() {
        let jenis = if lpk { "lpk" } else { "san" };
        for rows in b.sheets.values() {
            for x in rows.iter().filter(|x| norm_nama(&x.korfas) == kunci_korwil) {
                let kab = x.kab.clone().unwrap_or_default();
                let kec = x.kec.clone().unwrap_or_default();
                let tahap = x.tahap.clone().unwrap_or_default();
                let nama = if lpk { x.nama.clone() } else { String::new() };
                let kunci = store::kunci_lokasi(&kab, &kec, &x.desa, &tahap, &nama);
                if sudah.insert((jenis.to_string(), kunci.clone())) {
                    out.push(json!({
                        "jenis": jenis, "kunci": kunci, "kab": kab, "kec": kec,
                        "desa": x.desa, "tahap": tahap, "nama": nama,
                    }));
                }
            }
        }
    }
    out
}

/// Daftar lokasi dan isian tersimpan untuk satu korwil, beserta label kolom untuk form di halaman.
pub async fn daftar(State(st): State<AppState>, session: Session, Query(q): Query<KorwilQuery>) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    crate::lapming::api::butuh_lapming(&user)?;
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, q.korwil.as_deref()).await?;
    let isi = store::muat(&st.db, &korwil).await?;

    let lokasi_isi: BTreeMap<String, Value> = isi
        .lokasi
        .iter()
        .map(|((jenis, kunci), d)| (format!("{jenis}|{kunci}"), json!({ "tahapan": d.tahapan, "target": d.target })))
        .collect();
    Ok(Json(json!({
        "korwil": korwil,
        "lokasi": lokasi_lapming(&ds, &korwil),
        "isi": {
            "lokasi": lokasi_isi,
            "partisipan": isi.partisipan,
            "permasalahan": isi.permasalahan,
        },
        "kolom": {
            "tahap_san": kolom_json(&store::TAHAP_SAN),
            "tahap_lpk": kolom_json(&store::TAHAP_LPK),
            "target_san": kolom_json(&store::TARGET_SAN),
            "target_lpk": kolom_json(&store::TARGET_LPK),
            "partisipan": kolom_json(&store::PARTISIPAN_KOLOM),
            "permasalahan": kolom_json(&store::PERMASALAHAN_KOLOM),
        },
    })))
}

#[derive(Deserialize)]
pub struct SimpanLokasiIn {
    korwil: String,
    jenis: String,
    kunci: String,
    #[serde(default)]
    tahapan: BTreeMap<String, String>,
    #[serde(default)]
    target: BTreeMap<String, String>,
}

fn valid_tanggal(s: &str) -> bool {
    s.is_empty() || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

/// Simpan tanggal tahapan dan target satu lokasi. Nilai kosong menghapus isian.
pub async fn simpan_lokasi(State(st): State<AppState>, session: Session, Json(body): Json<SimpanLokasiIn>) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    crate::lapming::api::butuh_lapming(&user)?;
    if body.jenis != "san" && body.jenis != "lpk" {
        return Err(AppError::BadRequest("Jenis harus san atau lpk".into()));
    }
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, Some(&body.korwil)).await?;

    for (no, tgl) in &body.tahapan {
        let n: usize = no.parse().map_err(|_| AppError::BadRequest(format!("Nomor tahap tidak valid: {no}")))?;
        let batas = if body.jenis == "san" { store::TAHAP_SAN.len() } else { store::TAHAP_LPK.len() };
        if n == 0 || n > batas {
            return Err(AppError::BadRequest(format!("Nomor tahap di luar rentang: {no}")));
        }
        if !valid_tanggal(tgl) {
            return Err(AppError::BadRequest(format!("Tanggal tidak valid (YYYY-MM-DD): {tgl}")));
        }
    }
    let target_valid: BTreeSet<&str> = if body.jenis == "san" {
        store::TARGET_SAN.iter().map(|(k, _)| *k).collect()
    } else {
        store::TARGET_LPK.iter().map(|(k, _)| *k).collect()
    };
    if let Some(k) = body.target.keys().find(|k| !target_valid.contains(k.as_str())) {
        return Err(AppError::BadRequest(format!("Kolom target tidak dikenal: {k}")));
    }

    let data = store::LokasiData {
        tahapan: body.tahapan.into_iter().filter(|(_, v)| !v.is_empty()).collect(),
        target: body.target.into_iter().filter(|(_, v)| !v.trim().is_empty()).collect(),
    };
    store::simpan_lokasi(&st.db, &korwil, &body.jenis, &body.kunci, &data).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SimpanBarisIn {
    korwil: String,
    bagian: String,
    daftar: Vec<BTreeMap<String, String>>,
}

/// Ganti seluruh daftar partisipan atau permasalahan satu korwil.
pub async fn simpan_baris(State(st): State<AppState>, session: Session, Json(body): Json<SimpanBarisIn>) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    crate::lapming::api::butuh_lapming(&user)?;
    let kolom: BTreeSet<&str> = match body.bagian.as_str() {
        "partisipan" => store::PARTISIPAN_KOLOM.iter().map(|(k, _)| *k).collect(),
        "permasalahan" => store::PERMASALAHAN_KOLOM.iter().map(|(k, _)| *k).collect(),
        _ => return Err(AppError::BadRequest("Bagian harus partisipan atau permasalahan".into())),
    };
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, Some(&body.korwil)).await?;
    for baris in &body.daftar {
        if let Some(k) = baris.keys().find(|k| !kolom.contains(k.as_str())) {
            return Err(AppError::BadRequest(format!("Kolom tidak dikenal: {k}")));
        }
    }
    let daftar: Vec<BTreeMap<String, String>> = body
        .daftar
        .into_iter()
        .map(|b| b.into_iter().filter(|(_, v)| !v.trim().is_empty()).collect::<BTreeMap<_, _>>())
        .filter(|b| !b.is_empty())
        .collect();
    store::ganti_baris(&st.db, &korwil, &body.bagian, &daftar).await?;
    Ok(Json(json!({ "ok": true, "jumlah": daftar.len() })))
}

/// Impor form mingguan lama (.xlsx yang sudah diisi) untuk korwil yang dipilih.
/// Lokasi yang sama ditimpa; daftar partisipan dan permasalahan diganti seluruhnya.
pub async fn impor(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<KorwilQuery>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    lap_api::butuh_lapming(&user)?;
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, q.korwil.as_deref()).await?;
    let hasil = impor::baca(&body).map_err(AppError::BadRequest)?;
    for (jenis, kunci, data) in &hasil.lokasi {
        store::simpan_lokasi(&st.db, &korwil, jenis, kunci, data).await?;
    }
    store::ganti_baris(&st.db, &korwil, "partisipan", &hasil.partisipan).await?;
    store::ganti_baris(&st.db, &korwil, "permasalahan", &hasil.permasalahan).await?;
    Ok(Json(json!({
        "ok": true,
        "lokasi": hasil.lokasi.len(),
        "partisipan": hasil.partisipan.len(),
        "permasalahan": hasil.permasalahan.len(),
    })))
}
