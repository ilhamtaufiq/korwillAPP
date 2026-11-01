use std::collections::BTreeMap;

use axum::{extract::State, Json};
use serde::Serialize;
use tower_sessions::Session;

use crate::{auth::current_user, error::AppError, AppState};

// Satu pemanfaat punya 10 sel foto: 2 bangunan x 5 tahap.
const SEL_PER_PEMANFAAT: i64 = 10;
const FEED_LIMIT: i64 = 6;

#[derive(sqlx::FromRow)]
struct PekerjaanRow {
    id: u64,
    judul: String,
    pokmas: String,
    desa: String,
    kabupaten: String,
    tfl_nama: Option<String>,
    fm_pemberdayaan_nama: Option<String>,
    target_sasaran: u32,
    pemanfaat: i64,
    foto: i64,
}

#[derive(sqlx::FromRow)]
struct FeedRow {
    photo_id: u64,
    nama: String,
    building: String,
    stage: u8,
    project_id: u64,
    pokmas: String,
    tfl_nama: Option<String>,
    taken_at: Option<chrono::NaiveDateTime>,
}

#[derive(Serialize)]
pub struct Stats {
    pekerjaan: usize,
    /// Pekerjaan yang sudah punya pemanfaat.
    aktif: usize,
    /// Pekerjaan dengan semua sel foto terisi.
    lengkap: usize,
    pemanfaat: i64,
    target_kk: i64,
    foto_terisi: i64,
    sel_total: i64,
    persen: f64,
}

#[derive(Serialize)]
pub struct Pekerjaan {
    id: u64,
    judul: String,
    pokmas: String,
    desa: String,
    kabupaten: String,
    tfl_nama: Option<String>,
    fm_pemberdayaan_nama: Option<String>,
    pemanfaat: i64,
    foto: i64,
    sel_total: i64,
    persen: f64,
    /// "lengkap", "berjalan", atau "belum_mulai".
    status: &'static str,
}

#[derive(Serialize)]
pub struct Feed {
    photo_id: u64,
    nama: String,
    building: String,
    stage: u8,
    project_id: u64,
    pokmas: String,
    tfl_nama: Option<String>,
    taken_at: Option<chrono::NaiveDateTime>,
}

#[derive(Serialize)]
pub struct KabupatenStat {
    kabupaten: String,
    pekerjaan: usize,
    pemanfaat: i64,
    foto_terisi: i64,
    persen: f64,
}

#[derive(Serialize)]
pub struct Dashboard {
    stats: Stats,
    pekerjaan: Vec<Pekerjaan>,
    feed: Vec<Feed>,
    per_kabupaten: Vec<KabupatenStat>,
}

/// Persen foto terisi dari total sel, satu angka desimal. 0 kalau belum ada sel.
fn persen(terisi: i64, total: i64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (terisi as f64 / total as f64 * 1000.0).round() / 10.0
    }
}

/// Ringkasan untuk dashboard. Hanya menghitung pekerjaan yang boleh diakses user yang login.
pub async fn get(State(st): State<AppState>, session: Session) -> Result<Json<Dashboard>, AppError> {
    let user = current_user(&session, &st.db).await?;
    let is_admin = user.role == "admin";

    let rows = sqlx::query_as::<_, PekerjaanRow>(
        "SELECT p.id, p.judul, p.pokmas, p.desa, p.kabupaten, p.tfl_nama, p.fm_pemberdayaan_nama, p.target_sasaran, \
         (SELECT COUNT(*) FROM beneficiaries b WHERE b.project_id = p.id AND b.deleted_at IS NULL) AS pemanfaat, \
         (SELECT COUNT(*) FROM photos ph JOIN beneficiaries b ON b.id = ph.beneficiary_id \
          WHERE b.project_id = p.id AND b.deleted_at IS NULL AND ph.deleted_at IS NULL) AS foto \
         FROM projects p \
         WHERE p.deleted_at IS NULL AND (? OR p.id IN (SELECT project_id FROM project_members WHERE user_id = ?)) \
         ORDER BY p.id DESC",
    )
    .bind(is_admin)
    .bind(user.id)
    .fetch_all(&st.db)
    .await?;

    let feed_rows = sqlx::query_as::<_, FeedRow>(
        "SELECT ph.id AS photo_id, b.nama, CAST(ph.building AS CHAR) AS building, ph.stage, p.id AS project_id, p.pokmas, p.tfl_nama, ph.taken_at \
         FROM photos ph \
         JOIN beneficiaries b ON b.id = ph.beneficiary_id \
         JOIN projects p ON p.id = b.project_id \
         WHERE ph.deleted_at IS NULL AND b.deleted_at IS NULL AND p.deleted_at IS NULL \
         AND (? OR p.id IN (SELECT project_id FROM project_members WHERE user_id = ?)) \
         ORDER BY ph.taken_at DESC, ph.id DESC \
         LIMIT ?",
    )
    .bind(is_admin)
    .bind(user.id)
    .bind(FEED_LIMIT)
    .fetch_all(&st.db)
    .await?;

    let mut pemanfaat_total = 0i64;
    let mut foto_total = 0i64;
    let mut target_total = 0i64;
    let mut aktif = 0usize;
    let mut lengkap = 0usize;
    // Per kabupaten: (jumlah pekerjaan, pemanfaat, foto terisi). BTreeMap supaya urutannya tetap.
    let mut per_kab: BTreeMap<String, (usize, i64, i64)> = BTreeMap::new();

    let pekerjaan: Vec<Pekerjaan> = rows
        .iter()
        .map(|r| {
            let sel_total = r.pemanfaat * SEL_PER_PEMANFAAT;
            pemanfaat_total += r.pemanfaat;
            foto_total += r.foto;
            target_total += r.target_sasaran as i64;
            if r.pemanfaat > 0 {
                aktif += 1;
            }
            if r.pemanfaat > 0 && r.foto >= sel_total {
                lengkap += 1;
            }
            let entry = per_kab.entry(r.kabupaten.clone()).or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += r.pemanfaat;
            entry.2 += r.foto;

            let status = if r.foto == 0 {
                "belum_mulai"
            } else if r.pemanfaat > 0 && r.foto >= sel_total {
                "lengkap"
            } else {
                "berjalan"
            };
            Pekerjaan {
                id: r.id,
                judul: r.judul.clone(),
                pokmas: r.pokmas.clone(),
                desa: r.desa.clone(),
                kabupaten: r.kabupaten.clone(),
                tfl_nama: r.tfl_nama.clone(),
                fm_pemberdayaan_nama: r.fm_pemberdayaan_nama.clone(),
                pemanfaat: r.pemanfaat,
                foto: r.foto,
                sel_total,
                persen: persen(r.foto, sel_total),
                status,
            }
        })
        .collect();

    let sel_total = pemanfaat_total * SEL_PER_PEMANFAAT;
    let per_kabupaten = per_kab
        .into_iter()
        .map(|(kabupaten, (pekerjaan, pemanfaat, foto_terisi))| KabupatenStat {
            kabupaten,
            pekerjaan,
            pemanfaat,
            foto_terisi,
            persen: persen(foto_terisi, pemanfaat * SEL_PER_PEMANFAAT),
        })
        .collect();

    let feed = feed_rows
        .into_iter()
        .map(|r| Feed {
            photo_id: r.photo_id,
            nama: r.nama,
            building: r.building,
            stage: r.stage,
            project_id: r.project_id,
            pokmas: r.pokmas,
            tfl_nama: r.tfl_nama,
            taken_at: r.taken_at,
        })
        .collect();

    Ok(Json(Dashboard {
        stats: Stats {
            pekerjaan: rows.len(),
            aktif,
            lengkap,
            pemanfaat: pemanfaat_total,
            target_kk: target_total,
            foto_terisi: foto_total,
            sel_total,
            persen: persen(foto_total, sel_total),
        },
        pekerjaan,
        feed,
        per_kabupaten,
    }))
}
