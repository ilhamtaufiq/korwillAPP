use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::MySqlPool;
use tower_sessions::Session;

use crate::{
    auth::{current_user, User},
    error::AppError,
    members::link_tfl_by_name,
    AppState,
};

pub const PROJECT_COLS: &str = "p.id, p.judul, p.kegiatan, p.pokmas, p.desa, p.kecamatan, p.kabupaten, p.tahun, \
     p.kode_proyek, p.target_sasaran, p.korwil_nama, p.tfl_nama, p.pagu_dana";

#[derive(Serialize, sqlx::FromRow)]
pub struct Project {
    pub id: u64,
    pub judul: String,
    pub kegiatan: String,
    pub pokmas: String,
    pub desa: String,
    pub kecamatan: String,
    pub kabupaten: String,
    pub tahun: u16,
    pub kode_proyek: Option<String>,
    pub target_sasaran: u32,
    pub korwil_nama: Option<String>,
    pub tfl_nama: Option<String>,
    pub pagu_dana: Option<u64>,
}

#[derive(Deserialize)]
pub struct ProjectInput {
    judul: String,
    kegiatan: String,
    pokmas: String,
    desa: String,
    kecamatan: String,
    kabupaten: String,
    tahun: u16,
    kode_proyek: Option<String>,
    target_sasaran: u32,
    // Diisi dari impor Lapming. Form create tidak mengirimnya, jadi default kosong.
    #[serde(default)]
    korwil_nama: Option<String>,
    #[serde(default)]
    tfl_nama: Option<String>,
    #[serde(default)]
    pagu_dana: Option<u64>,
}

impl ProjectInput {
    fn validate(&self) -> Result<(), AppError> {
        require("judul", &self.judul, 255)?;
        require("kegiatan", &self.kegiatan, 255)?;
        require("pokmas", &self.pokmas, 255)?;
        require("desa", &self.desa, 128)?;
        require("kecamatan", &self.kecamatan, 128)?;
        require("kabupaten", &self.kabupaten, 128)?;
        if self.target_sasaran == 0 {
            return Err(AppError::BadRequest("Target sasaran harus lebih dari 0".to_string()));
        }
        if !(2000..=2100).contains(&self.tahun) {
            return Err(AppError::BadRequest("Tahun tidak valid".to_string()));
        }
        Ok(())
    }
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Beneficiary {
    pub id: u64,
    pub project_id: u64,
    pub no: u32,
    pub nama: String,
    pub rt_rw_dusun: String,
    pub nik: Option<String>,
}

#[derive(Deserialize)]
pub struct BeneficiaryInput {
    nama: String,
    rt_rw_dusun: String,
    nik: Option<String>,
}

impl BeneficiaryInput {
    fn validate(&self) -> Result<(), AppError> {
        require("nama", &self.nama, 255)?;
        if self.rt_rw_dusun.trim().chars().count() > 255 {
            return Err(AppError::BadRequest("RT/RW/Dusun terlalu panjang".to_string()));
        }
        if let Some(nik) = opt_trim(&self.nik) {
            if nik.len() != 16 || !nik.chars().all(|c| c.is_ascii_digit()) {
                return Err(AppError::BadRequest("NIK harus 16 digit angka".to_string()));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct MemberInput {
    user_id: u64,
}

/// Nilai opsional dari form: spasi dibuang, kosong dianggap tidak diisi.
fn opt_trim(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn require(field: &str, value: &str, max_chars: usize) -> Result<(), AppError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(AppError::BadRequest(format!("{field} wajib diisi")));
    }
    if v.chars().count() > max_chars {
        return Err(AppError::BadRequest(format!("{field} maksimal {max_chars} karakter")));
    }
    Ok(())
}

pub fn require_admin(user: &User) -> Result<(), AppError> {
    if user.role == "admin" {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// Admin boleh semua proyek. TFL hanya proyek yang ada di project_members.
/// Proyek yang tidak boleh diakses dibalas 404 agar keberadaannya tidak bocor.
pub async fn ensure_project_access(db: &MySqlPool, user: &User, project_id: u64) -> Result<(), AppError> {
    if user.role == "admin" {
        return Ok(());
    }
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?")
            .bind(project_id)
            .bind(user.id)
            .fetch_optional(db)
            .await?;
    found.map(|_| ()).ok_or(AppError::NotFound)
}

async fn fetch_project(db: &MySqlPool, id: u64) -> Result<Project, AppError> {
    sqlx::query_as::<_, Project>(&format!("SELECT {PROJECT_COLS} FROM projects p WHERE p.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn fetch_beneficiary(db: &MySqlPool, id: u64) -> Result<Beneficiary, AppError> {
    sqlx::query_as::<_, Beneficiary>(
        "SELECT id, project_id, no, nama, rt_rw_dusun, nik FROM beneficiaries WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

// ---- Proyek ----

pub async fn list(State(st): State<AppState>, session: Session) -> Result<Json<Vec<Project>>, AppError> {
    let user = current_user(&session, &st.db).await?;
    let rows = if user.role == "admin" {
        sqlx::query_as::<_, Project>(&format!("SELECT {PROJECT_COLS} FROM projects p ORDER BY p.id DESC"))
            .fetch_all(&st.db)
            .await?
    } else {
        sqlx::query_as::<_, Project>(&format!(
            "SELECT {PROJECT_COLS} FROM projects p \
             JOIN project_members m ON m.project_id = p.id \
             WHERE m.user_id = ? ORDER BY p.id DESC"
        ))
        .bind(user.id)
        .fetch_all(&st.db)
        .await?
    };
    Ok(Json(rows))
}

pub async fn create(
    State(st): State<AppState>,
    session: Session,
    Json(input): Json<ProjectInput>,
) -> Result<(StatusCode, Json<Project>), AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;
    input.validate()?;

    let res = sqlx::query(
        "INSERT INTO projects (judul, kegiatan, pokmas, desa, kecamatan, kabupaten, tahun, kode_proyek, target_sasaran) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(input.judul.trim())
    .bind(input.kegiatan.trim())
    .bind(input.pokmas.trim())
    .bind(input.desa.trim())
    .bind(input.kecamatan.trim())
    .bind(input.kabupaten.trim())
    .bind(input.tahun)
    .bind(opt_trim(&input.kode_proyek))
    .bind(input.target_sasaran)
    .execute(&st.db)
    .await?;

    let project = fetch_project(&st.db, res.last_insert_id()).await?;
    Ok((StatusCode::CREATED, Json(project)))
}

pub async fn get(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<Json<Project>, AppError> {
    let user = current_user(&session, &st.db).await?;
    ensure_project_access(&st.db, &user, id).await?;
    Ok(Json(fetch_project(&st.db, id).await?))
}

pub async fn update(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Project>, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;
    input.validate()?;
    fetch_project(&st.db, id).await?;

    sqlx::query(
        "UPDATE projects SET judul = ?, kegiatan = ?, pokmas = ?, desa = ?, kecamatan = ?, kabupaten = ?, tahun = ?, kode_proyek = ?, target_sasaran = ?, korwil_nama = ?, tfl_nama = ?, pagu_dana = ? WHERE id = ?",
    )
    .bind(input.judul.trim())
    .bind(input.kegiatan.trim())
    .bind(input.pokmas.trim())
    .bind(input.desa.trim())
    .bind(input.kecamatan.trim())
    .bind(input.kabupaten.trim())
    .bind(input.tahun)
    .bind(opt_trim(&input.kode_proyek))
    .bind(input.target_sasaran)
    .bind(opt_trim(&input.korwil_nama))
    .bind(opt_trim(&input.tfl_nama))
    .bind(input.pagu_dana)
    .bind(id)
    .execute(&st.db)
    .await?;

    // Nama TFL di form diganti: hubungkan akun TFL yang namanya cocok.
    if let Some(korwil) = opt_trim(&input.korwil_nama) {
        link_tfl_by_name(&st.db, id, &korwil).await?;
    }
    if let Some(tfl) = opt_trim(&input.tfl_nama) {
        link_tfl_by_name(&st.db, id, &tfl).await?;
    }
    Ok(Json(fetch_project(&st.db, id).await?))
}

pub async fn delete(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let res = sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(&st.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_member(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    Json(input): Json<MemberInput>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;
    fetch_project(&st.db, id).await?;

    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE id = ?")
        .bind(input.user_id)
        .fetch_optional(&st.db)
        .await?;
    if exists.is_none() {
        return Err(AppError::BadRequest("User tidak ditemukan".to_string()));
    }

    sqlx::query("INSERT IGNORE INTO project_members (project_id, user_id) VALUES (?, ?)")
        .bind(id)
        .bind(input.user_id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Pemanfaat ----

pub async fn list_beneficiaries(
    State(st): State<AppState>,
    session: Session,
    Path(project_id): Path<u64>,
) -> Result<Json<Vec<Beneficiary>>, AppError> {
    let user = current_user(&session, &st.db).await?;
    ensure_project_access(&st.db, &user, project_id).await?;

    let rows = sqlx::query_as::<_, Beneficiary>(
        "SELECT id, project_id, no, nama, rt_rw_dusun, nik FROM beneficiaries WHERE project_id = ? ORDER BY no",
    )
    .bind(project_id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

pub async fn create_beneficiary(
    State(st): State<AppState>,
    session: Session,
    Path(project_id): Path<u64>,
    Json(input): Json<BeneficiaryInput>,
) -> Result<(StatusCode, Json<Beneficiary>), AppError> {
    let user = current_user(&session, &st.db).await?;
    ensure_project_access(&st.db, &user, project_id).await?;
    input.validate()?;

    // Nomor urut otomatis: MAX + 1 dihitung dalam satu INSERT ... SELECT.
    let res = sqlx::query(
        "INSERT INTO beneficiaries (project_id, no, nama, rt_rw_dusun, nik) \
         SELECT ?, CAST(COALESCE(MAX(no), 0) AS SIGNED) + 1, ?, ?, ? FROM beneficiaries WHERE project_id = ?",
    )
    .bind(project_id)
    .bind(input.nama.trim())
    .bind(input.rt_rw_dusun.trim())
    .bind(opt_trim(&input.nik))
    .bind(project_id)
    .execute(&st.db)
    .await?;

    let row = fetch_beneficiary(&st.db, res.last_insert_id()).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn update_beneficiary(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    Json(input): Json<BeneficiaryInput>,
) -> Result<Json<Beneficiary>, AppError> {
    let user = current_user(&session, &st.db).await?;
    let current = fetch_beneficiary(&st.db, id).await?;
    ensure_project_access(&st.db, &user, current.project_id).await?;
    input.validate()?;

    sqlx::query("UPDATE beneficiaries SET nama = ?, rt_rw_dusun = ?, nik = ? WHERE id = ?")
        .bind(input.nama.trim())
        .bind(input.rt_rw_dusun.trim())
        .bind(opt_trim(&input.nik))
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(Json(fetch_beneficiary(&st.db, id).await?))
}

pub async fn delete_beneficiary(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    let current = fetch_beneficiary(&st.db, id).await?;
    ensure_project_access(&st.db, &user, current.project_id).await?;

    sqlx::query("DELETE FROM beneficiaries WHERE id = ?")
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Impor dari teks tempel (hasil copy Excel memakai tab, CSV memakai koma).
/// Setiap baris: `nama` lalu `rt_rw_dusun`. Baris pertama dilewati jika berisi header "nama".
pub async fn import_beneficiaries(
    State(st): State<AppState>,
    session: Session,
    Path(project_id): Path<u64>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    ensure_project_access(&st.db, &user, project_id).await?;

    let text = String::from_utf8(body.to_vec())
        .map_err(|_| AppError::BadRequest("Teks harus UTF-8".to_string()))?;

    let mut parsed: Vec<(String, String)> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let sep = if line.contains('\t') { '\t' } else { ',' };
        let mut parts = line.splitn(2, sep);
        let nama = parts.next().unwrap_or("").trim();
        let alamat = parts.next().unwrap_or("").trim();

        if i == 0 && nama.eq_ignore_ascii_case("nama") {
            continue;
        }
        if nama.is_empty() {
            return Err(AppError::BadRequest(format!("Baris {} tidak punya nama", i + 1)));
        }
        if nama.chars().count() > 255 || alamat.chars().count() > 255 {
            return Err(AppError::BadRequest(format!("Baris {} terlalu panjang", i + 1)));
        }
        parsed.push((nama.to_string(), alamat.to_string()));
    }

    // Satu transaksi: jika ada yang gagal, tidak ada pemanfaat setengah masuk.
    let mut tx = st.db.begin().await?;
    let mut next_no: i64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(no), 0) AS SIGNED) FROM beneficiaries WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_one(&mut *tx)
    .await?;

    for (nama, alamat) in &parsed {
        next_no += 1;
        sqlx::query("INSERT INTO beneficiaries (project_id, no, nama, rt_rw_dusun) VALUES (?, ?, ?, ?)")
            .bind(project_id)
            .bind(next_no)
            .bind(nama)
            .bind(alamat)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    Ok(Json(json!({ "imported": parsed.len() })))
}
