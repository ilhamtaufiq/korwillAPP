use std::{collections::HashSet, io::Cursor};

use axum::{
    body::Bytes,
    extract::{Query, State},
    Json,
};
use calamine::{open_workbook_auto_from_rs, Data, Range, Reader};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{MySql, Transaction};
use tower_sessions::Session;

use crate::{
    auth::{current_user, hash_password},
    error::{internal, AppError},
    members::link_tfl_by_name,
    projects::require_admin,
    AppState,
};

// Posisi kolom di sheet rekap Lapming (sama untuk file LPK dan Sanimas TA 2026).
const COL_KABUPATEN: usize = 3;
const COL_KECAMATAN: usize = 4;
const COL_DESA: usize = 5;
const COL_KMP: usize = 6;
const COL_KORFAS: usize = 7;
const COL_TFL: usize = 8;
const COL_PAGU: usize = 10;

#[derive(Deserialize)]
pub struct ImportQuery {
    kegiatan: String,
    tahun: u16,
}

/// Satu proyek (pokmas/KMP) hasil baca rekap Lapming.
struct LapmingRow {
    kabupaten: String,
    kecamatan: String,
    desa: String,
    pokmas: String,
    korwil: Option<String>,
    tfl: Option<String>,
    pagu: Option<u64>,
}

/// Akun yang dibuat dari impor. Password sama dengan username (sesuai permintaan).
#[derive(Serialize)]
struct NewAccount {
    username: String,
    password: String,
    role: String,
    nama: String,
}

/// Impor data awal pekerjaan dari file rekap Lapming (.xlsx). Hanya admin.
/// Setiap kombinasi kabupaten, kecamatan, desa, dan pokmas dibuat sekali.
/// Baris yang sudah ada untuk kegiatan dan tahun yang sama dilewati.
/// Akun korwil dan TFL dibuat otomatis jika belum ada. Username = kata pertama nama.
pub async fn import_xlsx(
    State(st): State<AppState>,
    session: Session,
    Query(query): Query<ImportQuery>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    require_admin(&user)?;

    let kegiatan = query.kegiatan.trim().to_string();
    if kegiatan.is_empty() || kegiatan.chars().count() > 255 {
        return Err(AppError::BadRequest("Kegiatan wajib diisi (maks. 255 karakter)".to_string()));
    }
    if !(2000..=2100).contains(&query.tahun) {
        return Err(AppError::BadRequest("Tahun tidak valid".to_string()));
    }
    if body.is_empty() {
        return Err(AppError::BadRequest("File kosong".to_string()));
    }

    let (rows, sheets) = tokio::task::spawn_blocking(move || read_lapming(&body))
        .await
        .map_err(internal)??;

    let mut tx = st.db.begin().await?;
    let mut inserted = 0u32;
    let mut already = 0u32;
    let mut invalid = 0u32;
    // Pasangan (proyek, nama korwil/TFL) yang dihubungkan setelah transaksi selesai.
    let mut links: Vec<(u64, String)> = Vec::new();
    let mut accounts: Vec<NewAccount> = Vec::new();
    let mut seen_usernames: HashSet<String> = HashSet::new();

    for row in &rows {
        if exceeds_limits(row) {
            invalid += 1;
            continue;
        }
        if let Some(nama) = &row.korwil {
            ensure_account(&mut tx, nama, "korwil", &mut seen_usernames, &mut accounts).await?;
        }
        if let Some(nama) = &row.tfl {
            ensure_account(&mut tx, nama, "tfl", &mut seen_usernames, &mut accounts).await?;
        }

        let exists: Option<u64> = sqlx::query_scalar(
            "SELECT id FROM projects WHERE kabupaten = ? AND kecamatan = ? AND desa = ? AND pokmas = ? AND kegiatan = ? AND tahun = ? LIMIT 1",
        )
        .bind(&row.kabupaten)
        .bind(&row.kecamatan)
        .bind(&row.desa)
        .bind(&row.pokmas)
        .bind(&kegiatan)
        .bind(query.tahun)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = exists {
            push_links(&mut links, id, row);
            already += 1;
            continue;
        }

        let judul: String = format!("Konstruksi {kegiatan} {}", row.pokmas).chars().take(255).collect();
        let res = sqlx::query(
            "INSERT INTO projects (judul, kegiatan, pokmas, desa, kecamatan, kabupaten, tahun, korwil_nama, tfl_nama, pagu_dana) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(judul)
        .bind(&kegiatan)
        .bind(&row.pokmas)
        .bind(&row.desa)
        .bind(&row.kecamatan)
        .bind(&row.kabupaten)
        .bind(query.tahun)
        .bind(&row.korwil)
        .bind(&row.tfl)
        .bind(row.pagu)
        .execute(&mut *tx)
        .await?;
        push_links(&mut links, res.last_insert_id(), row);
        inserted += 1;
    }
    tx.commit().await?;

    for (project_id, nama) in &links {
        link_tfl_by_name(&st.db, *project_id, nama).await?;
    }

    Ok(Json(json!({
        "sheet_dibaca": sheets,
        "baris_ditemukan": rows.len(),
        "ditambahkan": inserted,
        "sudah_ada": already,
        "tidak_valid": invalid,
        "akun_baru": accounts,
    })))
}

fn push_links(links: &mut Vec<(u64, String)>, project_id: u64, row: &LapmingRow) {
    if let Some(nama) = &row.korwil {
        links.push((project_id, nama.clone()));
    }
    if let Some(nama) = &row.tfl {
        links.push((project_id, nama.clone()));
    }
}

/// Membuat akun untuk satu nama kalau belum ada. Username = kata pertama nama, huruf kecil.
/// Password sama dengan username. Akun yang sudah ada di database tidak diubah.
async fn ensure_account(
    tx: &mut Transaction<'_, MySql>,
    nama: &str,
    role: &str,
    seen: &mut HashSet<String>,
    created: &mut Vec<NewAccount>,
) -> Result<(), AppError> {
    let Some(username) = account_username(nama) else {
        return Ok(());
    };
    if !seen.insert(username.clone()) {
        return Ok(());
    }

    let exists: Option<u64> = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&mut **tx)
        .await?;
    if exists.is_some() {
        return Ok(());
    }

    let hash = hash_password(&username).map_err(internal)?;
    // Password akun impor sama dengan username, jadi wajib diganti setelah login pertama.
    sqlx::query("INSERT INTO users (username, password_hash, role, nama, must_change_password) VALUES (?, ?, ?, ?, 1)")
        .bind(&username)
        .bind(hash)
        .bind(role)
        .bind(nama)
        .execute(&mut **tx)
        .await?;

    created.push(NewAccount {
        username: username.clone(),
        password: username,
        role: role.to_string(),
        nama: nama.to_string(),
    });
    Ok(())
}

/// Username dari nama: kata pertama, huruf kecil, hanya huruf dan angka. Contoh: "Ganthi Sulaiman" -> "ganthi".
/// Kalau nama lebih dari satu kata dan ada inisial (mis. "M. Iqbal"), pakai kata terakhir: "M. Iqbal" -> "iqbal".
fn account_username(nama: &str) -> Option<String> {
    let words: Vec<&str> = nama.split_whitespace().collect();
    let has_initial = words.len() > 1 && words.iter().any(|w| alnum(w).chars().count() == 1);
    let chosen = if has_initial { words.last()? } else { words.first()? };

    let username = alnum(chosen).to_lowercase();
    if username.is_empty() {
        None
    } else {
        Some(username)
    }
}

fn alnum(word: &str) -> String {
    word.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

#[cfg(test)]
mod tests {
    use super::account_username;

    #[test]
    fn username_uses_first_word_by_default() {
        assert_eq!(account_username("Ganthi Sulaiman").as_deref(), Some("ganthi"));
        assert_eq!(account_username("Rio Arista Hidayat, ST").as_deref(), Some("rio"));
    }

    #[test]
    fn username_uses_last_word_when_there_is_an_initial() {
        assert_eq!(account_username("M. Nur Taufiequl Ahyar").as_deref(), Some("ahyar"));
        assert_eq!(account_username("M. Iqbal").as_deref(), Some("iqbal"));
        assert_eq!(account_username("Ahmad M. Fauzi").as_deref(), Some("fauzi"));
    }

    #[test]
    fn empty_name_gives_no_username() {
        assert_eq!(account_username("   "), None);
    }
}

/// Membaca semua sheet rekap Lapming. Sheet mingguan (M1, M2, ...) berisi pokmas yang sama,
/// jadi hanya kemunculan pertama tiap pokmas yang diambil.
fn read_lapming(bytes: &[u8]) -> Result<(Vec<LapmingRow>, usize), AppError> {
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|_| AppError::BadRequest("File bukan Excel yang bisa dibaca (.xlsx)".to_string()))?;

    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let mut sheets = 0usize;

    for name in workbook.sheet_names() {
        let Ok(range) = workbook.worksheet_range(&name) else {
            continue;
        };
        if !is_lapming_sheet(&range) {
            continue;
        }
        sheets += 1;

        for cells in range.rows() {
            // Baris data punya nomor urut berupa angka di kolom pertama.
            if !matches!(cells.first(), Some(Data::Float(_)) | Some(Data::Int(_))) {
                continue;
            }
            let kabupaten = cell_text(cells, COL_KABUPATEN);
            let kecamatan = cell_text(cells, COL_KECAMATAN);
            let desa = cell_text(cells, COL_DESA);
            let pokmas = cell_text(cells, COL_KMP);
            if kabupaten.is_empty() || kecamatan.is_empty() || desa.is_empty() || pokmas.is_empty() || pokmas == "-" {
                continue;
            }

            let key = [&kabupaten, &kecamatan, &desa, &pokmas]
                .map(|s| s.to_lowercase())
                .join("|");
            if !seen.insert(key) {
                continue;
            }

            rows.push(LapmingRow {
                kabupaten,
                kecamatan,
                desa,
                pokmas,
                korwil: non_empty(cell_text(cells, COL_KORFAS)),
                tfl: non_empty(cell_text(cells, COL_TFL)),
                pagu: cell_number(cells, COL_PAGU),
            });
        }
    }

    if sheets == 0 {
        return Err(AppError::BadRequest(
            "Format tidak dikenali: sheet rekap Lapming tidak ditemukan".to_string(),
        ));
    }
    Ok((rows, sheets))
}

/// Sheet rekap punya header "Kabupaten/Kota" di kolom kabupaten dan "Nama KMP" di kolom KMP.
fn is_lapming_sheet(range: &Range<Data>) -> bool {
    range.rows().take(10).any(|cells| {
        cell_text(cells, COL_KABUPATEN).to_lowercase().contains("kabupaten")
            && cell_text(cells, COL_KMP).to_lowercase().contains("kmp")
    })
}

fn cell_text(cells: &[Data], idx: usize) -> String {
    let raw = match cells.get(idx) {
        Some(Data::String(s)) => s.clone(),
        Some(Data::Float(f)) => f.to_string(),
        Some(Data::Int(i)) => i.to_string(),
        _ => String::new(),
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cell_number(cells: &[Data], idx: usize) -> Option<u64> {
    match cells.get(idx) {
        Some(Data::Float(f)) if *f > 0.0 => Some(*f as u64),
        Some(Data::Int(i)) if *i > 0 => Some(*i as u64),
        _ => None,
    }
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() || s == "-" {
        None
    } else {
        Some(s)
    }
}

/// Batas panjang kolom di tabel projects. Baris yang melebihi dilewati, bukan menggagalkan impor.
fn exceeds_limits(row: &LapmingRow) -> bool {
    row.pokmas.chars().count() > 255
        || row.kabupaten.chars().count() > 128
        || row.kecamatan.chars().count() > 128
        || row.desa.chars().count() > 128
        || row.korwil.as_deref().is_some_and(|s| s.chars().count() > 255)
        || row.tfl.as_deref().is_some_and(|s| s.chars().count() > 255)
}
