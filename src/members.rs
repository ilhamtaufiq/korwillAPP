use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use sqlx::MySqlPool;
use tower_sessions::Session;

use crate::{
    auth::current_user,
    error::AppError,
    projects::require_admin,
    AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct Account {
    pub id: u64,
    pub username: String,
    pub nama: Option<String>,
}

/// Menghubungkan akun korwil atau TFL ke pekerjaan kalau namanya sama dengan kolom korwil/TFL di rekap.
/// Pencocokan tidak membedakan huruf besar dan spasi di awal/akhir. Tidak ada akun yang cocok = tidak terjadi apa-apa.
pub async fn link_tfl_by_name(db: &MySqlPool, project_id: u64, nama: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT IGNORE INTO project_members (project_id, user_id) \
         SELECT ?, id FROM users WHERE role IN ('tfl', 'korwil') AND LOWER(TRIM(nama)) = LOWER(TRIM(?))",
    )
    .bind(project_id)
    .bind(nama)
    .execute(db)
    .await?;
    Ok(())
}

/// Anggota (akun TFL) yang sudah terhubung ke pekerjaan ini.
pub async fn list(
    State(st): State<AppState>,
    session: Session,
    Path(project_id): Path<u64>,
) -> Result<Json<Vec<Account>>, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let rows = sqlx::query_as::<_, Account>(
        "SELECT u.id, u.username, u.nama FROM project_members m \
         JOIN users u ON u.id = m.user_id WHERE m.project_id = ? ORDER BY u.nama, u.username",
    )
    .bind(project_id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

pub async fn remove(
    State(st): State<AppState>,
    session: Session,
    Path((project_id, user_id)): Path<(u64, u64)>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let res = sqlx::query("DELETE FROM project_members WHERE project_id = ? AND user_id = ?")
        .bind(project_id)
        .bind(user_id)
        .execute(&st.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Daftar akun TFL untuk dipilih saat menambah anggota.
pub async fn list_tfl(State(st): State<AppState>, session: Session) -> Result<Json<Vec<Account>>, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let rows = sqlx::query_as::<_, Account>(
        "SELECT id, username, nama FROM users WHERE role IN ('tfl', 'korwil') ORDER BY nama, username",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}
