//! Checklist berkas per pokmas. Daftar pokmas diambil dari data Lapming untuk korwil yang login
//! (admin memilih korwil lewat query). Ceklis disimpan per pokmas di database.

use std::collections::{BTreeMap, HashSet};

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_sessions::Session;

use crate::{
    auth::current_user,
    berkas::BERKAS,
    error::AppError,
    laporan_mingguan::api::korwil_izin,
    lapming::{api as lap_api, fmt::norm_nama, model::Dataset},
    AppState,
};

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Info {
    pub no: String,
    pub tahap: String,
    pub kab: String,
    pub kec: String,
    pub desa: String,
    pub nama: String,
    pub korfas: String,
}

/// Kunci pokmas yang stabil: kabupaten, kecamatan, desa, dan nama pokmas (huruf kecil, tanpa spasi berlebih).
pub fn kunci_pokmas(kab: &str, kec: &str, desa: &str, nama: &str) -> String {
    format!("{}|{}|{}|{}", norm_nama(kab), norm_nama(kec), norm_nama(desa), norm_nama(nama))
}

/// Pokmas korwil dari data Lapming, tanpa duplikat, urut seperti di sumber.
fn pokmas_korwil(ds: &Dataset, korwil: &str) -> Vec<Info> {
    let kunci_korwil = norm_nama(korwil);
    let mut sudah: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (_lpk, b) in ds.books() {
        for rows in b.sheets.values() {
            for x in rows.iter().filter(|x| norm_nama(&x.korfas) == kunci_korwil) {
                let kab = x.kab.clone().unwrap_or_default();
                let kec = x.kec.clone().unwrap_or_default();
                let kunci = kunci_pokmas(&kab, &kec, &x.desa, &x.nama);
                if sudah.insert(kunci) {
                    out.push(Info {
                        no: x.no.clone(),
                        tahap: x.tahap.clone().unwrap_or_default(),
                        kab,
                        kec,
                        desa: x.desa.clone(),
                        nama: x.nama.clone(),
                        korfas: x.korfas.clone(),
                    });
                }
            }
        }
    }
    out
}

#[derive(Deserialize)]
pub struct KorwilQuery {
    korwil: Option<String>,
}

/// Pokmas korwil yang login (admin: korwil dari query) beserta ceklis yang sudah tersimpan.
pub async fn daftar(State(st): State<AppState>, session: Session, Query(q): Query<KorwilQuery>) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    lap_api::butuh_lapming(&user)?;
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, q.korwil.as_deref()).await?;

    let tersimpan: Vec<(String, String)> = sqlx::query_as("SELECT kunci, centang FROM checklist_pokmas WHERE korwil = ?")
        .bind(&korwil)
        .fetch_all(&st.db)
        .await?;
    let tersimpan: BTreeMap<String, BTreeMap<String, String>> = tersimpan
        .into_iter()
        .map(|(k, c)| (k, serde_json::from_str(&c).unwrap_or_default()))
        .collect();

    let pokmas: Vec<Value> = pokmas_korwil(&ds, &korwil)
        .into_iter()
        .map(|info| {
            let kunci = kunci_pokmas(&info.kab, &info.kec, &info.desa, &info.nama);
            let centang = tersimpan.get(&kunci).cloned().unwrap_or_default();
            json!({ "kunci": kunci, "info": info, "centang": centang })
        })
        .collect();

    Ok(Json(json!({
        "korwil": korwil,
        "items": BERKAS.iter().map(|(k, g, l)| json!({ "kode": k, "grup": g, "label": l })).collect::<Vec<_>>(),
        "pokmas": pokmas,
    })))
}

#[derive(Deserialize)]
pub struct CentangIn {
    korwil: Option<String>,
    kunci: String,
    kode: String,
    /// Kosong = hapus ceklis.
    nilai: String,
}

/// Mengubah satu ceklis pokmas. Admin wajib mengirim korwil.
pub async fn centang(State(st): State<AppState>, session: Session, Json(body): Json<CentangIn>) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    lap_api::butuh_lapming(&user)?;
    if !BERKAS.iter().any(|(k, _, _)| *k == body.kode) {
        return Err(AppError::BadRequest(format!("Kode berkas tidak dikenal: {}", body.kode)));
    }
    let ds = lap_api::muat_dataset(&st.db).await?;
    let korwil = korwil_izin(&st, &user, &ds, body.korwil.as_deref()).await?;
    // Pokmas harus ada di data Lapming korwil ini.
    let ada = pokmas_korwil(&ds, &korwil)
        .iter()
        .any(|i| kunci_pokmas(&i.kab, &i.kec, &i.desa, &i.nama) == body.kunci);
    if !ada {
        return Err(AppError::NotFound);
    }

    let lama: Option<String> = sqlx::query_scalar("SELECT centang FROM checklist_pokmas WHERE korwil = ? AND kunci = ?")
        .bind(&korwil)
        .bind(&body.kunci)
        .fetch_optional(&st.db)
        .await?;
    let mut centang: BTreeMap<String, String> = lama.map(|c| serde_json::from_str(&c).unwrap_or_default()).unwrap_or_default();
    let nilai = body.nilai.trim();
    if nilai.is_empty() {
        centang.remove(&body.kode);
    } else {
        centang.insert(body.kode.clone(), nilai.to_string());
    }
    sqlx::query(
        "INSERT INTO checklist_pokmas (korwil, kunci, centang) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE centang = VALUES(centang)",
    )
    .bind(&korwil)
    .bind(&body.kunci)
    .bind(serde_json::to_string(&centang).map_err(|e| AppError::BadRequest(e.to_string()))?)
    .execute(&st.db)
    .await?;
    Ok(Json(json!({ "ok": true, "centang": centang })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lapming::{model::Dataset, read};

    #[test]
    fn pokmas_korwil_diambil_dari_data_lapming() {
        let dir = "/root/.claude/uploads/ed2718b6-4e6a-595b-9618-806d016befd8/";
        let (Ok(san), Ok(lpk)) = (
            std::fs::read(format!("{dir}ff3da0cf-Lapming_Konstruksi_Sanimas_-_TA_2026_1.xlsx")),
            std::fs::read(format!("{dir}5b44c9db-Lapming_Konstruksi_Sanitasi_LPK_1.xlsx")),
        ) else {
            return;
        };
        let ds = Dataset {
            san: Some(read::baca_book(&san, false).unwrap()),
            lpk: Some(read::baca_book(&lpk, true).unwrap()),
            ..Default::default()
        };
        let daftar = pokmas_korwil(&ds, "Ganthi");
        assert_eq!(daftar.len(), 16, "14 Sanimas dan 2 LPK");
        assert!(daftar.iter().any(|p| p.nama == "Sugema Jaya" && p.desa == "Cibarusah Jaya"));
        assert!(pokmas_korwil(&ds, "Tidak Ada").is_empty());
    }
}
