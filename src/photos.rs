use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use image::{imageops::FilterType, ColorType, DynamicImage};
use axum::http::HeaderMap;
use serde::Serialize;
use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}};
use sqlx::MySqlPool;
use tower_sessions::Session;

use crate::{
    auth::current_user,
    error::{internal, AppError},
    projects::ensure_project_access,
    riwayat::{self, AKSI_CREATE, AKSI_DELETE, AKSI_UPDATE, ENTITAS_FOTO},
    AppState,
};

pub const UPLOAD_DIR: &str = "uploads";
pub const BUILDINGS: [&str; 2] = ["bilik", "tangki_septik"];
pub const STAGES: [u8; 5] = [0, 25, 50, 75, 100];

const FULL_PX: u32 = 1280;
const THUMB_PX: u32 = 400;

#[derive(Serialize, sqlx::FromRow)]
pub struct PhotoRow {
    pub id: u64,
    pub beneficiary_id: u64,
    pub building: String,
    pub stage: u8,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// Koordinat GPS dari perangkat, dikirim sebagai query ?lat=&lng=. Boleh tidak ada.
#[derive(serde::Deserialize)]
pub struct GeoQuery {
    lat: Option<f64>,
    lng: Option<f64>,
}

pub async fn beneficiary_project_id(db: &MySqlPool, beneficiary_id: u64) -> Result<u64, AppError> {
    sqlx::query_scalar("SELECT project_id FROM beneficiaries WHERE id = ? AND deleted_at IS NULL")
        .bind(beneficiary_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

fn validate_cell(building: &str, stage: u8) -> Result<(), AppError> {
    if !BUILDINGS.contains(&building) || !STAGES.contains(&stage) {
        return Err(AppError::BadRequest("Sel foto tidak valid".to_string()));
    }
    Ok(())
}

/// Daftar semua foto dalam satu proyek. Thumbnail diambil lewat /api/photos/:id/thumb.
pub async fn list(
    State(st): State<AppState>,
    session: Session,
    Path(project_id): Path<u64>,
) -> Result<Json<Vec<PhotoRow>>, AppError> {
    let user = current_user(&session, &st.db).await?;
    ensure_project_access(&st.db, &user, project_id).await?;

    let rows = sqlx::query_as::<_, PhotoRow>(
        "SELECT ph.id, ph.beneficiary_id, CAST(ph.building AS CHAR) AS building, ph.stage, ph.latitude, ph.longitude \
         FROM photos ph JOIN beneficiaries b ON b.id = ph.beneficiary_id \
         WHERE b.project_id = ? AND b.deleted_at IS NULL AND ph.deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

/// Unggah foto untuk satu sel. Body berisi bytes gambar mentah (JPEG atau PNG).
/// Unggah ulang ke sel yang sama menimpa foto lama.
pub async fn upload(
    State(st): State<AppState>,
    session: Session,
    Path((beneficiary_id, building, stage)): Path<(u64, String, u8)>,
    Query(geo): Query<GeoQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<PhotoRow>), AppError> {
    let user = current_user(&session, &st.db).await?;
    let project_id = beneficiary_project_id(&st.db, beneficiary_id).await?;
    ensure_project_access(&st.db, &user, project_id).await?;
    validate_cell(&building, stage)?;
    if body.is_empty() {
        return Err(AppError::BadRequest("Foto kosong".to_string()));
    }

    // Decode dan encode gambar berat, jadi dijalankan di thread blocking.
    let (full, thumb) = tokio::task::spawn_blocking(move || process(&body))
        .await
        .map_err(internal)??;

    let dir = format!("{UPLOAD_DIR}/{project_id}");
    tokio::fs::create_dir_all(&dir).await.map_err(internal)?;
    let path = format!("{dir}/{beneficiary_id}-{building}-{stage}.jpg");
    let thumb_path = format!("{dir}/{beneficiary_id}-{building}-{stage}-thumb.jpg");
    tokio::fs::write(&path, &full).await.map_err(internal)?;
    tokio::fs::write(&thumb_path, &thumb).await.map_err(internal)?;

    // Sel yang sudah berisi foto aktif diperbarui. Sel yang kosong atau foto hapusannya dibuat baru.
    let sebelumnya: Option<(u64, i64)> = sqlx::query_as(
        "SELECT id, CAST(deleted_at IS NOT NULL AS SIGNED) FROM photos WHERE beneficiary_id = ? AND building = ? AND stage = ?",
    )
    .bind(beneficiary_id)
    .bind(&building)
    .bind(stage)
    .fetch_optional(&st.db)
    .await?;

    sqlx::query(
        "INSERT INTO photos (beneficiary_id, building, stage, path, thumb_path, taken_at, uploaded_by, latitude, longitude) \
         VALUES (?, ?, ?, ?, ?, NOW(), ?, ?, ?) \
         ON DUPLICATE KEY UPDATE path = VALUES(path), thumb_path = VALUES(thumb_path), \
         taken_at = VALUES(taken_at), uploaded_by = VALUES(uploaded_by), \
         latitude = VALUES(latitude), longitude = VALUES(longitude), deleted_at = NULL",
    )
    .bind(beneficiary_id)
    .bind(&building)
    .bind(stage)
    .bind(&path)
    .bind(&thumb_path)
    .bind(user.id)
    .bind(geo.lat.filter(|v| (-90.0..=90.0).contains(v)))
    .bind(geo.lng.filter(|v| (-180.0..=180.0).contains(v)))
    .execute(&st.db)
    .await?;

    let row = sqlx::query_as::<_, PhotoRow>(
        "SELECT id, beneficiary_id, CAST(building AS CHAR) AS building, stage, latitude, longitude FROM photos \
         WHERE beneficiary_id = ? AND building = ? AND stage = ?",
    )
    .bind(beneficiary_id)
    .bind(&building)
    .bind(stage)
    .fetch_one(&st.db)
    .await?;

    let (aksi, sudah_ada) = match sebelumnya {
        Some((_, 0)) => (AKSI_UPDATE, true),
        _ => (AKSI_CREATE, false),
    };
    let (_, text) = riwayat::label(&st.db, ENTITAS_FOTO, row.id).await?;
    let keterangan = if sudah_ada {
        format!("{text} diganti")
    } else {
        format!("{text} diunggah")
    };
    riwayat::catat(&st.db, &user, aksi, ENTITAS_FOTO, row.id, Some(project_id), &keterangan).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn delete(
    State(st): State<AppState>,
    session: Session,
    Path((beneficiary_id, building, stage)): Path<(u64, String, u8)>,
) -> Result<StatusCode, AppError> {
    let user = current_user(&session, &st.db).await?;
    let project_id = beneficiary_project_id(&st.db, beneficiary_id).await?;
    ensure_project_access(&st.db, &user, project_id).await?;
    validate_cell(&building, stage)?;

    let photo_id: Option<u64> = sqlx::query_scalar(
        "SELECT id FROM photos WHERE beneficiary_id = ? AND building = ? AND stage = ? AND deleted_at IS NULL",
    )
    .bind(beneficiary_id)
    .bind(&building)
    .bind(stage)
    .fetch_optional(&st.db)
    .await?;
    let Some(photo_id) = photo_id else {
        return Err(AppError::NotFound);
    };

    // Hapus lunak: file di disk dibiarkan supaya foto bisa dipulihkan dari riwayat.
    let (_, text) = riwayat::label(&st.db, ENTITAS_FOTO, photo_id).await?;
    sqlx::query("UPDATE photos SET deleted_at = NOW() WHERE id = ?")
        .bind(photo_id)
        .execute(&st.db)
        .await?;
    riwayat::catat(
        &st.db,
        &user,
        AKSI_DELETE,
        ENTITAS_FOTO,
        photo_id,
        Some(project_id),
        &format!("{text} dihapus"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Membalas bytes foto dengan ETag dari isinya. Sel yang diunggah ulang setelah dihapus memakai id yang sama,
/// jadi URL-nya tidak berubah. Karena itu browser harus memeriksa ulang (no-cache) dan hanya memakai
/// salinan lama jika ETag-nya masih sama (304).
fn respons_foto(bytes: Vec<u8>, headers: &HeaderMap) -> Response {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let etag = format!("\"{:016x}\"", hasher.finish());

    let cocok = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
    let umum = [
        (header::CACHE_CONTROL, "private, no-cache"),
        (header::ETAG, etag.as_str()),
    ];
    if cocok {
        return (StatusCode::NOT_MODIFIED, umum).into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "private, no-cache"),
            (header::ETAG, etag.as_str()),
        ],
        bytes,
    )
        .into_response()
}

/// Foto ukuran penuh untuk pratinjau. Aksesnya diperiksa seperti thumbnail.
pub async fn image(
    State(st): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    let user = current_user(&session, &st.db).await?;
    let row: Option<(u64, String)> = sqlx::query_as(
        "SELECT b.project_id, ph.path FROM photos ph \
         JOIN beneficiaries b ON b.id = ph.beneficiary_id WHERE ph.id = ? AND ph.deleted_at IS NULL AND b.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let Some((project_id, path)) = row else {
        return Err(AppError::NotFound);
    };
    ensure_project_access(&st.db, &user, project_id).await?;

    let bytes = tokio::fs::read(&path).await.map_err(|_| AppError::NotFound)?;
    Ok(respons_foto(bytes, &headers))
}

/// Thumbnail satu foto. Tetap diperiksa aksesnya karena foto tidak boleh terbuka tanpa login.
pub async fn thumb(
    State(st): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    let user = current_user(&session, &st.db).await?;
    let row: Option<(u64, String)> = sqlx::query_as(
        "SELECT b.project_id, ph.thumb_path FROM photos ph \
         JOIN beneficiaries b ON b.id = ph.beneficiary_id WHERE ph.id = ? AND ph.deleted_at IS NULL AND b.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let Some((project_id, thumb_path)) = row else {
        return Err(AppError::NotFound);
    };
    ensure_project_access(&st.db, &user, project_id).await?;

    let bytes = tokio::fs::read(&thumb_path).await.map_err(|_| AppError::NotFound)?;
    Ok(respons_foto(bytes, &headers))
}

/// Foto disimpan utuh (tidak dipotong), hanya diperkecil jika terlalu besar. Hasilnya versi besar dan thumbnail (JPEG).
fn process(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), AppError> {
    let img = image::load_from_memory(bytes).map_err(|_| {
        AppError::BadRequest("Format foto tidak didukung. Gunakan JPEG atau PNG.".to_string())
    })?;
    // Foto HP sering disimpan miring dan hanya diberi tahu lewat EXIF. Diputar dulu sebelum crop.
    let img = apply_orientation(img, read_orientation(bytes));
    let full = encode_jpeg(&fit(&img, FULL_PX), 85)?;
    let thumb = encode_jpeg(&fit(&img, THUMB_PX), 80)?;
    Ok((full, thumb))
}

/// Nilai orientasi EXIF (1-8). Tanpa EXIF atau tidak terbaca, dianggap 1 (tidak diputar).
fn read_orientation(bytes: &[u8]) -> u32 {
    let mut cursor = std::io::Cursor::new(bytes);
    exif::Reader::new()
        .read_from_container(&mut cursor)
        .ok()
        .and_then(|data| data.get_field(exif::Tag::Orientation, exif::In::PRIMARY).and_then(|f| f.value.get_uint(0)))
        .unwrap_or(1)
}

/// Menerapkan orientasi EXIF: rotasi dan/atau cermin sesuai standar EXIF 1-8.
fn apply_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// Mengecilkan gambar hanya jika lebih besar dari batas. Tidak pernah membesarkan.
fn fit(img: &DynamicImage, max_px: u32) -> DynamicImage {
    if img.width() > max_px {
        img.resize(max_px, max_px, FilterType::Triangle)
    } else {
        img.clone()
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, AppError> {
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), ColorType::Rgb8)
        .map_err(internal)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image::GenericImageView;

    fn landscape() -> DynamicImage {
        DynamicImage::new_rgb8(4, 2)
    }

    #[test]
    fn orientation_6_rotates_landscape_to_portrait() {
        let img = apply_orientation(landscape(), 6);
        assert_eq!((img.width(), img.height()), (2, 4));
    }

    #[test]
    fn orientation_1_and_3_keep_dimensions() {
        assert_eq!(apply_orientation(landscape(), 1).dimensions(), (4, 2));
        assert_eq!(apply_orientation(landscape(), 3).dimensions(), (4, 2));
    }

    #[test]
    fn foto_diunggah_ulang_dikenali_lewat_etag() {
        let kosong = HeaderMap::new();
        let lama = respons_foto(b"foto lama".to_vec(), &kosong);
        let etag = lama.headers().get(header::ETAG).unwrap().clone();
        assert_eq!(lama.status(), StatusCode::OK);
        assert_eq!(lama.headers().get(header::CACHE_CONTROL).unwrap(), "private, no-cache");

        // Browser yang masih punya salinan lama: isi sama -> 304.
        let mut minta = HeaderMap::new();
        minta.insert(header::IF_NONE_MATCH, etag.clone());
        assert_eq!(respons_foto(b"foto lama".to_vec(), &minta).status(), StatusCode::NOT_MODIFIED);

        // Foto baru di id yang sama -> ETag beda -> 200 dengan isi baru.
        let baru = respons_foto(b"foto baru".to_vec(), &minta);
        assert_eq!(baru.status(), StatusCode::OK);
        assert_ne!(baru.headers().get(header::ETAG).unwrap(), &etag);
    }

    #[test]
    fn missing_exif_is_treated_as_orientation_1() {
        assert_eq!(read_orientation(b"bukan gambar"), 1);
    }
}
