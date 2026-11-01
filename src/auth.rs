use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tower_sessions::Session;

use crate::{
    error::{internal, AppError},
    AppState,
};

const SESSION_USER_KEY: &str = "user_id";

#[derive(Serialize, sqlx::FromRow, Clone)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub role: String,
    pub must_change_password: bool,
}

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    old_password: String,
    new_password: String,
}

#[derive(Deserialize)]
pub struct LoginBody {
    username: String,
    password: String,
}

pub async fn login(
    State(st): State<AppState>,
    session: Session,
    Json(body): Json<LoginBody>,
) -> Result<Json<User>, AppError> {
    let row: Option<(u64, String, String, String, bool)> = sqlx::query_as(
        "SELECT id, username, password_hash, CAST(role AS CHAR), must_change_password FROM users WHERE username = ?",
    )
    .bind(&body.username)
    .fetch_optional(&st.db)
    .await?;

    let Some((id, username, hash, role, must_change_password)) = row else {
        return Err(AppError::Unauthorized);
    };
    if !verify_password(&body.password, &hash) {
        return Err(AppError::Unauthorized);
    }

    // Ganti ID sesi setelah login untuk mencegah session fixation.
    session.cycle_id().await.map_err(internal)?;
    session.insert(SESSION_USER_KEY, id).await.map_err(internal)?;
    Ok(Json(User {
        id,
        username,
        role,
        must_change_password,
    }))
}

pub async fn logout(session: Session) -> Result<StatusCode, AppError> {
    session.flush().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(State(st): State<AppState>, session: Session) -> Result<Json<User>, AppError> {
    Ok(Json(current_user(&session, &st.db).await?))
}

/// Ganti kata sandi sendiri. Kata sandi lama harus benar, dan kata sandi baru tidak boleh sama dengan username.
pub async fn change_password(
    State(st): State<AppState>,
    session: Session,
    Json(body): Json<ChangePasswordBody>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    if body.new_password.chars().count() < 8 {
        return Err(AppError::BadRequest("Kata sandi baru minimal 8 karakter".to_string()));
    }
    if body.new_password == user.username {
        return Err(AppError::BadRequest("Kata sandi baru tidak boleh sama dengan username".to_string()));
    }

    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&st.db)
        .await?;
    if !verify_password(&body.old_password, &hash) {
        return Err(AppError::BadRequest("Kata sandi lama salah".to_string()));
    }

    let new_hash = hash_password(&body.new_password).map_err(internal)?;
    sqlx::query("UPDATE users SET password_hash = ?, must_change_password = 0 WHERE id = ?")
        .bind(new_hash)
        .bind(user.id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Mengambil user dari sesi. Dipanggil di awal setiap handler yang butuh login.
pub async fn current_user(session: &Session, db: &MySqlPool) -> Result<User, AppError> {
    let id: u64 = session
        .get(SESSION_USER_KEY)
        .await
        .map_err(internal)?
        .ok_or(AppError::Unauthorized)?;

    sqlx::query_as::<_, User>(
        "SELECT id, username, CAST(role AS CHAR) AS role, must_change_password FROM users WHERE id = ?",
    )
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::Unauthorized)
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(hash.to_string())
}

fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// `create-user <username> <admin|tfl>`. Password dibaca dari stdin agar tidak masuk riwayat shell.
pub async fn create_user_cli(db: &MySqlPool, args: &[String]) -> anyhow::Result<()> {
    let (username, role, nama) = match args {
        [username, role] => (username, role, None),
        [username, role, nama @ ..] => (username, role, Some(nama.join(" "))),
        _ => anyhow::bail!("Pakai: create-user <username> <admin|tfl> [nama lengkap]  (password dibaca dari stdin)"),
    };
    if !matches!(role.as_str(), "admin" | "tfl" | "korwil") {
        anyhow::bail!("Role harus admin, tfl, atau korwil");
    }

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let password = line.trim_end_matches(['\r', '\n']);
    if password.len() < 8 {
        anyhow::bail!("Password minimal 8 karakter");
    }

    let hash = hash_password(password)?;
    sqlx::query("INSERT INTO users (username, password_hash, role, nama) VALUES (?, ?, ?, ?)")
        .bind(username)
        .bind(hash)
        .bind(role)
        .bind(nama.as_deref())
        .execute(db)
        .await?;
    println!("User dibuat: {username} ({role})");
    Ok(())
}
