use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tower_sessions::Session;

use crate::{
    auth::{current_user, User},
    error::AppError,
    projects::require_admin,
    AppState,
};

pub const AKSI_CREATE: &str = "create";
pub const AKSI_UPDATE: &str = "update";
pub const AKSI_DELETE: &str = "delete";
pub const AKSI_RESTORE: &str = "restore";

pub const ENTITAS_PEKERJAAN: &str = "pekerjaan";
pub const ENTITAS_PEMANFAAT: &str = "pemanfaat";
pub const ENTITAS_FOTO: &str = "foto";

const PER_HALAMAN: u64 = 50;

/// Mencatat satu perubahan ke riwayat. Dipanggil di dalam handler setelah perubahan berhasil.
/// Bisa dipakai dengan pool biasa maupun dengan transaksi.
pub async fn catat<'e, E>(
    executor: E,
    user: &User,
    aksi: &str,
    entitas: &str,
    entitas_id: u64,
    project_id: Option<u64>,
    keterangan: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::MySql>,
{
    let keterangan: String = keterangan.chars().take(500).collect();
    sqlx::query(
        "INSERT INTO log_dokumentasi (user_id, username, aksi, entitas, entitas_id, project_id, keterangan) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user.id)
    .bind(&user.username)
    .bind(aksi)
    .bind(entitas)
    .bind(entitas_id)
    .bind(project_id)
    .bind(keterangan)
    .execute(executor)
    .await?;
    Ok(())
}

/// Nama data untuk keterangan riwayat, beserta id pekerjaan induknya. Data yang sudah dihapus tetap dibaca.
pub async fn label(db: &MySqlPool, entitas: &str, id: u64) -> Result<(u64, String), AppError> {
    let row: Option<(u64, String)> = match entitas {
        ENTITAS_PEKERJAAN => sqlx::query_as::<_, (u64, String)>("SELECT id, judul FROM projects WHERE id = ?")
            .bind(id)
            .fetch_optional(db)
            .await?
            .map(|(id, judul)| (id, format!("Pekerjaan '{judul}'"))),
        ENTITAS_PEMANFAAT => sqlx::query_as::<_, (u64, String)>("SELECT project_id, nama FROM beneficiaries WHERE id = ?")
            .bind(id)
            .fetch_optional(db)
            .await?
            .map(|(project_id, nama)| (project_id, format!("Pemanfaat '{nama}'"))),
        ENTITAS_FOTO => sqlx::query_as::<_, (u64, String, String, u8)>(
            "SELECT b.project_id, b.nama, CAST(ph.building AS CHAR), ph.stage FROM photos ph \
             JOIN beneficiaries b ON b.id = ph.beneficiary_id WHERE ph.id = ?",
        )
        .bind(id)
        .fetch_optional(db)
        .await?
        .map(|(project_id, nama, building, stage)| (project_id, format!("Foto {nama} {building} {stage}%"))),
        _ => None,
    };
    row.ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
pub struct ListQuery {
    halaman: Option<u64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct LogRow {
    pub id: u64,
    pub waktu: String,
    pub username: String,
    pub aksi: String,
    pub entitas: String,
    pub entitas_id: u64,
    pub project_id: Option<u64>,
    pub keterangan: String,
    /// 1 kalau data yang dimaksud masih dalam keadaan terhapus, berarti bisa dipulihkan.
    pub terhapus: i64,
}

#[derive(Serialize)]
pub struct LogPage {
    total: i64,
    halaman: u64,
    per_halaman: u64,
    items: Vec<LogRow>,
}

/// Riwayat perubahan, terbaru dulu. Khusus admin.
pub async fn list(
    State(st): State<AppState>,
    session: Session,
    Query(query): Query<ListQuery>,
) -> Result<Json<LogPage>, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let halaman = query.halaman.unwrap_or(1).max(1);
    let offset = (halaman - 1) * PER_HALAMAN;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM log_dokumentasi")
        .fetch_one(&st.db)
        .await?;

    let items = sqlx::query_as::<_, LogRow>(
        "SELECT l.id, DATE_FORMAT(l.waktu, '%Y-%m-%d %H:%i') AS waktu, l.username, \
         CAST(l.aksi AS CHAR) AS aksi, CAST(l.entitas AS CHAR) AS entitas, l.entitas_id, l.project_id, l.keterangan, \
         CAST((CASE l.entitas \
             WHEN 'pekerjaan' THEN (SELECT p.deleted_at FROM projects p WHERE p.id = l.entitas_id) \
             WHEN 'pemanfaat' THEN (SELECT b.deleted_at FROM beneficiaries b WHERE b.id = l.entitas_id) \
             ELSE (SELECT ph.deleted_at FROM photos ph WHERE ph.id = l.entitas_id) \
         END) IS NOT NULL AS SIGNED) AS terhapus \
         FROM log_dokumentasi l ORDER BY l.id DESC LIMIT ? OFFSET ?",
    )
    .bind(PER_HALAMAN as i64)
    .bind(offset as i64)
    .fetch_all(&st.db)
    .await?;

    Ok(Json(LogPage {
        total,
        halaman,
        per_halaman: PER_HALAMAN,
        items,
    }))
}

/// Memulihkan data yang dihapus lunak. Khusus admin.
pub async fn restore(
    State(st): State<AppState>,
    session: Session,
    Path((entitas, id)): Path<(String, u64)>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    // Nama tabel hanya dari daftar tetap di bawah, jadi aman dimasukkan ke SQL.
    let table = match entitas.as_str() {
        ENTITAS_PEKERJAAN => "projects",
        ENTITAS_PEMANFAAT => "beneficiaries",
        ENTITAS_FOTO => "photos",
        _ => return Err(AppError::BadRequest("Jenis data tidak dikenal".to_string())),
    };

    let status: Option<i64> =
        sqlx::query_scalar(&format!("SELECT CAST(deleted_at IS NOT NULL AS SIGNED) FROM {table} WHERE id = ?"))
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    match status {
        None => return Err(AppError::NotFound),
        Some(0) => return Err(AppError::BadRequest("Data ini tidak sedang dihapus".to_string())),
        Some(_) => {}
    }

    let (project_id, text) = label(&st.db, &entitas, id).await?;
    let mut tx = st.db.begin().await?;
    sqlx::query(&format!("UPDATE {table} SET deleted_at = NULL WHERE id = ?"))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    catat(
        &mut *tx,
        &user,
        AKSI_RESTORE,
        &entitas,
        id,
        Some(project_id),
        &format!("{text} dipulihkan"),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
