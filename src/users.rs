use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::{
    auth::{current_user, hash_password},
    error::{internal, AppError},
    projects::require_admin,
    AppState,
};

const ROLES: [&str; 3] = ["admin", "korwil", "tfl"];

#[derive(Serialize, sqlx::FromRow)]
pub struct UserRow {
    id: u64,
    username: String,
    nama: Option<String>,
    role: String,
    must_change_password: bool,
    pekerjaan: i64,
}

#[derive(Deserialize)]
pub struct CreateBody {
    username: String,
    nama: Option<String>,
    role: String,
    password: String,
}

#[derive(Deserialize)]
pub struct UpdateBody {
    nama: Option<String>,
    role: String,
}

#[derive(Deserialize)]
pub struct PasswordBody {
    password: String,
}

const USER_SELECT: &str = "SELECT u.id, u.username, u.nama, CAST(u.role AS CHAR) AS role, u.must_change_password, \
     (SELECT COUNT(*) FROM project_members m WHERE m.user_id = u.id) AS pekerjaan \
     FROM users u";

/// Daftar seluruh akun untuk admin. Kata sandi tidak pernah ikut dikirim.
pub async fn list(State(st): State<AppState>, session: Session) -> Result<Json<Vec<UserRow>>, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let rows = sqlx::query_as::<_, UserRow>(&format!(
        "{USER_SELECT} ORDER BY FIELD(u.role, 'admin', 'korwil', 'tfl'), u.nama, u.username"
    ))
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

/// Tambah akun baru. Akun baru ditandai wajib ganti kata sandi saat login pertama.
pub async fn create(
    State(st): State<AppState>,
    session: Session,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<UserRow>), AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let username = body.username.trim().to_lowercase();
    let username_ok = (3..=64).contains(&username.chars().count())
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !username_ok {
        return Err(AppError::BadRequest(
            "Username 3-64 karakter: huruf, angka, titik, garis bawah, atau strip".to_string(),
        ));
    }
    check_role(&body.role)?;
    check_password(&body.password)?;

    let exists: Option<u64> = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&st.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict("Username sudah dipakai".to_string()));
    }

    let hash = hash_password(&body.password).map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO users (username, password_hash, role, nama, must_change_password) VALUES (?, ?, ?, ?, 1)",
    )
    .bind(&username)
    .bind(hash)
    .bind(&body.role)
    .bind(clean_nama(body.nama))
    .execute(&st.db)
    .await?;

    let row = fetch_row(&st.db, res.last_insert_id()).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

/// Ubah nama dan peran. Username tidak bisa diubah. Admin tidak bisa menurunkan dirinya sendiri
/// atau admin terakhir, supaya sistem tidak kehilangan admin.
pub async fn update(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<UserRow>, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;
    check_role(&body.role)?;

    let target = fetch_row(&st.db, id).await?;
    if target.role == "admin" && body.role != "admin" {
        if target.id == user.id {
            return Err(AppError::BadRequest("Peran akun sendiri tidak bisa diturunkan".to_string()));
        }
        let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
            .fetch_one(&st.db)
            .await?;
        if admins <= 1 {
            return Err(AppError::BadRequest("Tidak bisa menurunkan admin terakhir".to_string()));
        }
    }

    sqlx::query("UPDATE users SET nama = ?, role = ? WHERE id = ?")
        .bind(clean_nama(body.nama))
        .bind(&body.role)
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(Json(fetch_row(&st.db, id).await?))
}

/// Admin membuat kata sandi baru untuk user. User diminta menggantinya saat login berikutnya.
pub async fn reset_password(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    Json(body): Json<PasswordBody>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;
    check_password(&body.password)?;
    fetch_row(&st.db, id).await?;

    let hash = hash_password(&body.password).map_err(internal)?;
    sqlx::query("UPDATE users SET password_hash = ?, must_change_password = 1 WHERE id = ?")
        .bind(hash)
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_row(db: &sqlx::MySqlPool, id: u64) -> Result<UserRow, AppError> {
    sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE u.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

fn check_role(role: &str) -> Result<(), AppError> {
    if ROLES.contains(&role) {
        Ok(())
    } else {
        Err(AppError::BadRequest("Peran tidak valid".to_string()))
    }
}

fn check_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < 8 {
        Err(AppError::BadRequest("Kata sandi minimal 8 karakter".to_string()))
    } else {
        Ok(())
    }
}

fn clean_nama(nama: Option<String>) -> Option<String> {
    nama.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
