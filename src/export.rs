use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{
    auth::current_user,
    error::{internal, AppError},
    pdf::{self, Pemanfaat, Photo, RenderOptions},
    photos::{BUILDINGS, STAGES},
    projects::{ensure_project_access, Beneficiary, Project},
    AppState,
};

const CATATAN_MAX_CHARS: usize = 150;

/// Opsi ekspor dari panel di halaman detail. Semua opsional.
#[derive(Deserialize)]
pub struct ExportQuery {
    #[serde(default = "default_true")]
    sel_kosong: bool,
    catatan: Option<String>,
    /// true untuk pratinjau di halaman. false (default) untuk unduhan.
    #[serde(default)]
    inline: bool,
}

fn default_true() -> bool {
    true
}

/// Ekspor PDF dokumentasi (A4 landscape) untuk satu proyek.
pub async fn export_pdf(
    State(st): State<AppState>,
    session: Session,
    Path(project_id): Path<u64>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let user = current_user(&session, &st.db).await?;
    ensure_project_access(&st.db, &user, project_id).await?;

    let project = sqlx::query_as::<_, Project>(
        "SELECT id, judul, kegiatan, pokmas, desa, kecamatan, kabupaten, tahun, kode_proyek, target_sasaran, korwil_nama, tfl_nama, fm_pemberdayaan_nama, pagu_dana FROM projects WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let beneficiaries = sqlx::query_as::<_, Beneficiary>(
        "SELECT id, project_id, no, nama, rt_rw_dusun, nik FROM beneficiaries WHERE project_id = ? AND deleted_at IS NULL ORDER BY no",
    )
    .bind(project_id)
    .fetch_all(&st.db)
    .await?;

    let photo_rows: Vec<(u64, String, u8, String)> = sqlx::query_as(
        "SELECT ph.beneficiary_id, CAST(ph.building AS CHAR), ph.stage, ph.thumb_path \
         FROM photos ph JOIN beneficiaries b ON b.id = ph.beneficiary_id WHERE b.project_id = ? AND b.deleted_at IS NULL AND ph.deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(&st.db)
    .await?;

    let mut rows: Vec<Pemanfaat> = beneficiaries
        .iter()
        .map(|b| Pemanfaat {
            no: b.no,
            nama: b.nama.clone(),
            rt_rw_dusun: b.rt_rw_dusun.clone(),
            nik: b.nik.clone(),
            foto: Default::default(),
        })
        .collect();
    let index: HashMap<u64, usize> = beneficiaries
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();

    // Sel yang file-nya hilang dibiarkan kosong, tidak menggagalkan seluruh ekspor.
    for (beneficiary_id, building, stage, thumb_path) in photo_rows {
        let Some(&row) = index.get(&beneficiary_id) else { continue };
        let Some(b) = BUILDINGS.iter().position(|x| *x == building) else { continue };
        let Some(s) = STAGES.iter().position(|x| *x == stage) else { continue };
        if let Ok(bytes) = tokio::fs::read(&thumb_path).await {
            rows[row].foto[b][s] = Some(Photo { jpeg: bytes });
        }
    }

    let judul = "Dokumentasi Pelaksanaan Konstruksi Sanimas".to_string();
    // Target hanya ditampilkan kalau sudah diisi (pekerjaan hasil impor masih 0).
    let target = if project.target_sasaran > 0 {
        format!(" | Target {} KK", project.target_sasaran)
    } else {
        String::new()
    };
    let info = format!(
        "{} | Desa {}, Kec. {}, Kab. {} | TA {}{}",
        project.pokmas, project.desa, project.kecamatan, project.kabupaten, project.tahun, target
    );

    let disposition = if query.inline { "inline" } else { "attachment" };
    let opts = RenderOptions {
        show_empty: query.sel_kosong,
        catatan: query
            .catatan
            .unwrap_or_default()
            .chars()
            .take(CATATAN_MAX_CHARS)
            .collect(),
        tahun: project.tahun,
    };

    // Render PDF cukup berat, jadi jalan di thread blocking.
    let pdf_bytes = tokio::task::spawn_blocking(move || pdf::render(&judul, &info, &rows, &opts))
        .await
        .map_err(internal)?
        .map_err(internal)?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("{disposition}; filename=\"dokumentasi-pekerjaan-{project_id}.pdf\""),
            ),
        ],
        pdf_bytes,
    )
        .into_response())
}
