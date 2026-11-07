//! Endpoint Lapming: impor file sumber ke database, laporan (xlsx dan pptx), log perubahan, RPDB, dan alias.

use std::collections::{BTreeMap, HashMap};

use axum::{
    body::Bytes,
    extract::{Multipart, Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{MySql, MySqlPool, QueryBuilder};
use tower_sessions::Session;

use super::calc::{self, DataPokmas, TahapRpdb};
use super::fmt::{norm_nama, periode};
use super::log::{self, LogEntri, Ubah};
use super::model::{Book, Dataset, Row};
use super::{pptx, read, xlsx};
use crate::{
    auth::{current_user, User},
    error::{internal, AppError},
    AppState,
};

const LOG_BATAS: usize = 5000;
const PER_HALAMAN: usize = 50;

/// Laporan Lapming untuk admin dan korwil.
fn butuh_lapming(user: &User) -> Result<(), AppError> {
    if user.role == "admin" || user.role == "korwil" {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

fn jenis_kode(lpk: bool) -> &'static str {
    if lpk {
        "lpk"
    } else {
        "san"
    }
}

/// Membaca semua data mingguan dari database menjadi Dataset.
async fn muat_dataset(db: &MySqlPool) -> Result<Dataset, AppError> {
    let sheets: Vec<(String, String)> = sqlx::query_as("SELECT CAST(jenis AS CHAR), nama FROM lapming_sheet")
        .fetch_all(db)
        .await?;
    let baris: Vec<(String, String, String)> =
        sqlx::query_as("SELECT CAST(jenis AS CHAR), sheet, data FROM lapming_data ORDER BY id")
            .fetch_all(db)
            .await?;
    let mut san = Book::default();
    let mut lpk = Book::default();
    let (mut ada_san, mut ada_lpk) = (false, false);
    for (j, nama) in sheets {
        let b = if j == "lpk" {
            ada_lpk = true;
            &mut lpk
        } else {
            ada_san = true;
            &mut san
        };
        b.names.push(nama);
    }
    for b in [&mut san, &mut lpk] {
        b.names.sort_by_key(|n| n.trim_start_matches('M').parse::<u32>().unwrap_or(0));
    }
    for (j, sheet, data) in baris {
        let x: Row = serde_json::from_str(&data).map_err(internal)?;
        let Some(minggu) = sheet.trim_start_matches('M').parse::<u32>().ok() else { continue };
        let b = if j == "lpk" { &mut lpk } else { &mut san };
        b.sheets.entry(minggu).or_default().push(x);
    }
    Ok(Dataset { san: ada_san.then_some(san), lpk: ada_lpk.then_some(lpk) })
}

/// Alias nama dan RPDB dari database.
async fn muat_pokmas(db: &MySqlPool) -> Result<DataPokmas, AppError> {
    let alias: Vec<(String, String)> = sqlx::query_as("SELECT nama_norm, nama_baku FROM lapming_alias")
        .fetch_all(db)
        .await?;
    let rpdb: Vec<(String, Option<i64>, Option<NaiveDate>)> =
        sqlx::query_as("SELECT nama_norm, jumlah, tanggal FROM lapming_rpdb ORDER BY nama_norm, tahap_no")
            .fetch_all(db)
            .await?;
    let mut data = DataPokmas { alias: alias.into_iter().collect(), ..Default::default() };
    for (k, jumlah, tanggal) in rpdb {
        data.rpdb.entry(k).or_default().push(TahapRpdb { jumlah, tanggal });
    }
    Ok(data)
}

/// Log perubahan satu korfas, terbaru dulu. Mengembalikan daftar (maksimal `batas`) dan jumlah seluruhnya.
async fn muat_log(db: &MySqlPool, korfas: &str, batas: usize) -> Result<(Vec<LogEntri>, usize), AppError> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lapming_log WHERE LOWER(TRIM(korfas)) = LOWER(TRIM(?))")
        .bind(korfas)
        .fetch_one(db)
        .await?;
    let baris: Vec<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT DATE_FORMAT(waktu, '%Y-%m-%d %H:%i:%s'), CAST(jenis AS CHAR), sheet, no_key, pokmas, korfas, ubah \
         FROM lapming_log WHERE LOWER(TRIM(korfas)) = LOWER(TRIM(?)) ORDER BY waktu DESC, id DESC LIMIT ?",
    )
    .bind(korfas)
    .bind(batas as i64)
    .fetch_all(db)
    .await?;
    let mut out = Vec::new();
    for (waktu, jenis, sheet, no, pokmas, kf, ubah) in baris {
        let ubah: Vec<Ubah> = serde_json::from_str(&ubah).map_err(internal)?;
        out.push(LogEntri { waktu, jenis, sheet, no, pokmas, korfas: kf, ubah });
    }
    Ok((out, total as usize))
}

/// Data mingguan hanya untuk korfas tertentu. Cukup untuk cek kualitas satu pekerjaan, tanpa membaca seluruh data.
async fn muat_dataset_korfas(db: &MySqlPool, korfas: &[String]) -> Result<Dataset, AppError> {
    if korfas.is_empty() {
        return Ok(Dataset::default());
    }
    let mut qb: QueryBuilder<MySql> =
        QueryBuilder::new("SELECT CAST(jenis AS CHAR), sheet, data FROM lapming_data WHERE korfas IN (");
    {
        let mut sep = qb.separated(", ");
        for k in korfas {
            sep.push_bind(k.clone());
        }
    }
    qb.push(") ORDER BY id");
    let baris: Vec<(String, String, String)> = qb.build_query_as().fetch_all(db).await?;
    let mut san = Book::default();
    let mut lpk = Book::default();
    let (mut ada_san, mut ada_lpk) = (false, false);
    for (j, sheet, data) in baris {
        let x: Row = serde_json::from_str(&data).map_err(internal)?;
        let Some(minggu) = sheet.trim_start_matches('M').parse::<u32>().ok() else { continue };
        let b = if j == "lpk" {
            ada_lpk = true;
            &mut lpk
        } else {
            ada_san = true;
            &mut san
        };
        b.sheets.entry(minggu).or_default().push(x);
    }
    Ok(Dataset { san: ada_san.then_some(san), lpk: ada_lpk.then_some(lpk) })
}

/// Mengisi `nama_norm` untuk baris lama (yang masih ''). Nama kosong ditandai '#kosong' supaya tidak diulang.
async fn isi_nama_norm_kosong(db: &MySqlPool) -> Result<(), AppError> {
    loop {
        let baris: Vec<(u64, String)> =
            sqlx::query_as("SELECT id, data FROM lapming_data WHERE nama_norm = '' LIMIT 1000").fetch_all(db).await?;
        if baris.is_empty() {
            return Ok(());
        }
        for (id, data) in baris {
            let x: Row = serde_json::from_str(&data).map_err(internal)?;
            let norm = norm_nama(&x.nama);
            let norm = if norm.is_empty() { "#kosong".to_string() } else { norm };
            sqlx::query("UPDATE lapming_data SET nama_norm = ? WHERE id = ?")
                .bind(norm)
                .bind(id)
                .execute(db)
                .await?;
        }
    }
}

/// Nama Pokmas setelah alias diterapkan, dalam bentuk ternormalisasi (sama dengan efektif di endpoint proyek).
fn efektif_nama(alias: &HashMap<String, String>, nama: &str) -> String {
    let k = norm_nama(nama);
    match alias.get(&k) {
        Some(baku) => norm_nama(baku),
        None => k,
    }
}

#[derive(Deserialize)]
pub struct ImporQuery {
    jenis: String,
    /// Tahun anggaran untuk pekerjaan yang dibuat dari impor Sanimas (admin). Kosong = tidak membuat pekerjaan.
    tahun: Option<u16>,
}

/// Mengimpor satu file sumber (Sanimas atau LPK) ke database. Perubahan dibanding pemuatan sebelumnya dicatat.
pub async fn impor(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<ImporQuery>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let lpk = jenis_lpk(&q.jenis)?;
    proses_impor(&st, &user, lpk, q.tahun, body).await
}

#[derive(Deserialize)]
pub struct LinkIn {
    link: String,
}

/// Mengimpor file sumber dari link Google Sheets atau Drive yang dibagikan ke siapa saja yang memiliki link.
pub async fn impor_link(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<ImporQuery>,
    Json(input): Json<LinkIn>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let lpk = jenis_lpk(&q.jenis)?;
    let isi = unduh_link(&input.link).await?;
    proses_impor(&st, &user, lpk, q.tahun, Bytes::from(isi)).await
}

fn jenis_lpk(jenis: &str) -> Result<bool, AppError> {
    match jenis {
        "san" => Ok(false),
        "lpk" => Ok(true),
        _ => Err(AppError::BadRequest("Jenis sumber harus san atau lpk".to_string())),
    }
}

const BATAS_UNDUH: usize = 30 * 1024 * 1024;

/// Mengunduh xlsx dari link Google Sheets atau Drive. URL unduhan dibentuk dari ID file, bukan dari link
/// pengguna, sehingga hanya host Google yang pernah dihubungi.
async fn unduh_link(link: &str) -> Result<Vec<u8>, AppError> {
    let re = regex::Regex::new(r"/d/([\w-]{20,})|[?&]id=([\w-]{20,})").unwrap();
    let id = re
        .captures(link.trim())
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| AppError::BadRequest("ID file tidak ditemukan di link. Pakai link Google Sheets atau Drive.".to_string()))?;
    let klien = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(internal)?;
    for url in [
        format!("https://drive.google.com/uc?export=download&id={id}"),
        format!("https://docs.google.com/spreadsheets/d/{id}/export?format=xlsx"),
    ] {
        let Ok(res) = klien.get(&url).send().await else { continue };
        if !res.status().is_success() {
            continue;
        }
        let Ok(b) = res.bytes().await else { continue };
        if b.len() > BATAS_UNDUH {
            return Err(AppError::BadRequest("File dari link terlalu besar (maks. 30 MB)".to_string()));
        }
        // xlsx adalah zip; halaman login atau galat Google bukan zip.
        if b.starts_with(b"PK") {
            return Ok(b.to_vec());
        }
    }
    Err(AppError::BadRequest(
        "Gagal mengunduh dari link. Pastikan file dibagikan sebagai \"Siapa saja yang memiliki link\", atau unggah file xlsx dari komputer.".to_string(),
    ))
}

async fn proses_impor(
    st: &AppState,
    user: &User,
    lpk: bool,
    tahun: Option<u16>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("File kosong".to_string()));
    }
    // Salinan untuk pembuatan pekerjaan di akhir, karena `body` sudah dipindahkan ke pembacaan file.
    let salinan = body.clone();
    let book = tokio::task::spawn_blocking(move || read::baca_book(&body, lpk))
        .await
        .map_err(internal)?
        .map_err(AppError::BadRequest)?;

    let lama = muat_dataset(&st.db).await?;
    let lama_book = if lpk { lama.lpk } else { lama.san };
    let punya_lama = lama_book.as_ref().map_or(false, |b| b.sheets.values().any(|r| !r.is_empty()));
    let baru = if lpk { Dataset { lpk: Some(book.clone()), ..Default::default() } } else { Dataset { san: Some(book.clone()), ..Default::default() } };
    // Impor pertama: semua baris dicatat sebagai "ditambahkan" (pembanding kosong).
    let entri: Vec<LogEntri> = if punya_lama {
        let lama_ds = if lpk { Dataset { lpk: lama_book, ..Default::default() } } else { Dataset { san: lama_book, ..Default::default() } };
        log::bandingkan(&log::snapshot(&lama_ds), &log::snapshot(&baru), "")
    } else {
        log::bandingkan(&Default::default(), &log::snapshot(&baru), "")
    };

    let jenis = jenis_kode(lpk);
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM lapming_data WHERE jenis = ?").bind(jenis).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM lapming_sheet WHERE jenis = ?").bind(jenis).execute(&mut *tx).await?;
    for n in &book.names {
        sqlx::query("INSERT INTO lapming_sheet (jenis, nama) VALUES (?, ?)").bind(jenis).bind(n).execute(&mut *tx).await?;
    }
    let semua: Vec<&Row> = book.sheets.values().flatten().collect();
    for potong in semua.chunks(200) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new("INSERT INTO lapming_data (jenis, sheet, no_key, korfas, data, nama_norm) ");
        let mut json_baris = Vec::with_capacity(potong.len());
        for x in potong {
            json_baris.push(serde_json::to_string(x).map_err(internal)?);
        }
        qb.push_values(potong.iter().zip(json_baris.iter()), |mut b, (x, j)| {
            b.push_bind(jenis).push_bind(&x.sheet).push_bind(&x.no).push_bind(&x.korfas).push_bind(j).push_bind(norm_nama(&x.nama));
        });
        qb.build().execute(&mut *tx).await?;
    }
    for e in &entri {
        sqlx::query("INSERT INTO lapming_log (jenis, sheet, no_key, pokmas, korfas, ubah) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&e.jenis)
            .bind(&e.sheet)
            .bind(&e.no)
            .bind(&e.pokmas)
            .bind(&e.korfas)
            .bind(serde_json::to_string(&e.ubah).map_err(internal)?)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    // Impor Sanimas oleh admin juga membuat pekerjaan dari rekap yang sama, kegiatan "Sanimas".
    let pekerjaan = match tahun {
        Some(t) if !lpk && user.role == "admin" => {
            Some(crate::import::buat_pekerjaan(&st.db, user, salinan, "Sanimas", t).await?)
        }
        _ => None,
    };

    Ok(Json(json!({
        "sheet_dibaca": book.sheets.len(),
        "sheet_total": book.names.len(),
        "baris": semua.len(),
        "pertama_kali": !punya_lama,
        "perubahan": entri.len(),
        "pekerjaan": pekerjaan,
    })))
}

#[derive(Deserialize)]
pub struct InfoQuery {
    korfas: Option<String>,
}

/// Pilihan untuk form laporan: korfas, minggu, dan tahap yang ada di database.
pub async fn info(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<InfoQuery>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let ds = muat_dataset(&st.db).await?;
    let korfas = calc::daftar_korfas(&ds);
    let minggu: Vec<String> = calc::daftar_sheet(&ds).iter().map(|n| format!("M{n}")).collect();
    let tahap = match q.korfas.as_deref().filter(|k| !k.is_empty()) {
        Some(k) => calc::daftar_tahap(&ds, k),
        None => Vec::new(),
    };
    let terakhir: Vec<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT CAST(jenis AS CHAR), DATE_FORMAT(MAX(updated_at), '%Y-%m-%d %H:%i'), COUNT(*) FROM lapming_data GROUP BY jenis",
    )
    .fetch_all(&st.db)
    .await?;
    let sumber: BTreeMap<String, Value> = terakhir
        .into_iter()
        .map(|(j, t, n)| (j, json!({ "diperbarui": t, "baris": n })))
        .collect();
    Ok(Json(json!({ "korfas": korfas, "minggu": minggu, "tahap": tahap, "sumber": sumber })))
}

#[derive(Deserialize)]
pub struct LaporanQuery {
    korfas: String,
    sheet: Option<String>,
    tahap: Option<String>,
}

impl LaporanQuery {
    fn pilih(&self) -> Option<u32> {
        self.sheet
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.trim().trim_start_matches(['M', 'm']).parse().ok())
    }

    fn tahap(&self) -> Option<&str> {
        self.tahap.as_deref().filter(|t| !t.trim().is_empty())
    }
}

/// Ringkasan sebelum membuat laporan.
pub async fn pratinjau(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<LaporanQuery>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let ds = muat_dataset(&st.db).await?;
    let baris = calc::pratinjau(&ds, &q.korfas, q.pilih(), q.tahap()).map_err(AppError::BadRequest)?;
    Ok(Json(json!({ "baris": baris })))
}

fn aman(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect::<String>().trim().to_string()
}

fn berkas(isi: Vec<u8>, tipe: &'static str, nama: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, tipe.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{nama}\"")),
        ],
        isi,
    )
        .into_response()
}

/// Membuat laporan dari Dataset yang diberikan, lalu mengembalikan xlsx atau pptx.
async fn buat_laporan(st: &AppState, ds: Dataset, q: LaporanQuery, format: &str) -> Result<Response, AppError> {
    if format != "xlsx" && format != "pptx" {
        return Err(AppError::NotFound);
    }
    let pokmas = muat_pokmas(&st.db).await?;
    let (entri, total) = muat_log(&st.db, &q.korfas, LOG_BATAS).await?;
    let format = format.to_string();
    let hasil = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, String, &'static str), String> {
        let lap = calc::susun(&ds, &q.korfas, q.pilih(), q.tahap(), &pokmas)?;
        let kf = aman(&q.korfas);
        let sfx = q.tahap().map(|t| format!(" - {}", aman(t))).unwrap_or_default();
        if format == "xlsx" {
            let isi = xlsx::tulis_gabungan(&ds, &lap, &q.korfas, q.tahap(), &pokmas.alias, &entri).map_err(|e| e.to_string())?;
            Ok((
                isi,
                format!("Lapming Konstruksi Sanimas dan LPK - {kf}{sfx}.xlsx"),
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ))
        } else {
            let sh = format!("M{}", lap.week);
            let potong = &entri[..entri.len().min(30)];
            let isi = pptx::slides::build(&lap.rows, &lap.s, lap.week, &sh, &q.korfas, Some((potong, total)), q.tahap())?;
            Ok((
                isi,
                format!("Lapming Sanimas dan LPK - {kf} - {sh}{sfx}.pptx"),
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ))
        }
    })
    .await
    .map_err(internal)?
    .map_err(AppError::BadRequest)?;
    Ok(berkas(hasil.0, hasil.2, hasil.1))
}

/// Laporan dari data di database. format = xlsx atau pptx.
pub async fn laporan(
    State(st): State<AppState>,
    session: Session,
    Path(format): Path<String>,
    Query(q): Query<LaporanQuery>,
) -> Result<Response, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let ds = muat_dataset(&st.db).await?;
    buat_laporan(&st, ds, q, &format).await
}

/// Laporan langsung dari file yang diunggah (tanpa menyimpan ke database). Field: sanimas, lpk, korfas, sheet, tahap.
pub async fn laporan_file(
    State(st): State<AppState>,
    session: Session,
    Path(format): Path<String>,
    mut mp: Multipart,
) -> Result<Response, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let (mut san, mut lpk) = (None, None);
    let (mut link_san, mut link_lpk) = (String::new(), String::new());
    let (mut korfas, mut sheet, mut tahap) = (String::new(), None, None);
    while let Some(f) = mp.next_field().await.map_err(|e| AppError::BadRequest(e.to_string()))? {
        let nama = f.name().unwrap_or("").to_string();
        match nama.as_str() {
            "sanimas" | "lpk" => {
                let b = f.bytes().await.map_err(|e| AppError::BadRequest(e.to_string()))?;
                if !b.is_empty() {
                    if nama == "lpk" {
                        lpk = Some(b);
                    } else {
                        san = Some(b);
                    }
                }
            }
            "link_sanimas" => link_san = f.text().await.map_err(|e| AppError::BadRequest(e.to_string()))?,
            "link_lpk" => link_lpk = f.text().await.map_err(|e| AppError::BadRequest(e.to_string()))?,
            "korfas" => korfas = f.text().await.map_err(|e| AppError::BadRequest(e.to_string()))?,
            "sheet" => sheet = Some(f.text().await.map_err(|e| AppError::BadRequest(e.to_string()))?),
            "tahap" => tahap = Some(f.text().await.map_err(|e| AppError::BadRequest(e.to_string()))?),
            _ => {}
        }
    }
    if san.is_none() && !link_san.trim().is_empty() {
        san = Some(Bytes::from(unduh_link(&link_san).await?));
    }
    if lpk.is_none() && !link_lpk.trim().is_empty() {
        lpk = Some(Bytes::from(unduh_link(&link_lpk).await?));
    }
    if san.is_none() && lpk.is_none() {
        return Err(AppError::BadRequest("Pilih minimal satu file atau link sumber (Sanimas atau LPK)".to_string()));
    }
    if korfas.trim().is_empty() {
        return Err(AppError::BadRequest("Korfas wajib diisi".to_string()));
    }
    let ds = tokio::task::spawn_blocking(move || -> Result<Dataset, String> {
        let mut ds = Dataset::default();
        if let Some(b) = san {
            ds.san = Some(read::baca_book(&b, false)?);
        }
        if let Some(b) = lpk {
            ds.lpk = Some(read::baca_book(&b, true)?);
        }
        Ok(ds)
    })
    .await
    .map_err(internal)?
    .map_err(AppError::BadRequest)?;
    buat_laporan(&st, ds, LaporanQuery { korfas: korfas.trim().to_string(), sheet, tahap }, &format).await
}
#[derive(Deserialize)]
pub struct LogQuery {
    korfas: String,
    halaman: Option<usize>,
}

/// Riwayat perubahan data untuk satu korfas, 50 per halaman, terbaru dulu.
pub async fn log_daftar(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<LogQuery>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let (entri, total) = muat_log(&st.db, &q.korfas, LOG_BATAS).await?;
    let halaman = q.halaman.unwrap_or(1).max(1);
    let awal = (halaman - 1) * PER_HALAMAN;
    let items: Vec<Value> = entri
        .iter()
        .skip(awal)
        .take(PER_HALAMAN)
        .map(|e| {
            json!({
                "waktu": e.waktu,
                "sumber": if e.jenis == "lpk" { "LPK" } else { "Sanimas" },
                "sheet": e.sheet,
                "pokmas": e.pokmas,
                "perubahan": log::ringkas_ubah(&e.ubah, 4),
            })
        })
        .collect();
    Ok(Json(json!({ "total": total, "halaman": halaman, "per_halaman": PER_HALAMAN, "items": items })))
}

#[derive(Serialize)]
struct TahapOut {
    jumlah: Option<i64>,
    tanggal: Option<String>,
}

/// Daftar Pokmas minggu terpilih beserta RPDB-nya, untuk form Data RPDB.
pub async fn pokmas_daftar(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<LaporanQuery>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let ds = muat_dataset(&st.db).await?;
    let pokmas = muat_pokmas(&st.db).await?;
    let (week, rows, _) = calc::siapkan(&ds, &q.korfas, q.pilih(), q.tahap()).map_err(AppError::BadRequest)?;
    let items: Vec<Value> = rows
        .iter()
        .map(|x| {
            let nama = pokmas.alias.get(&norm_nama(&x.nama)).cloned().unwrap_or_else(|| x.nama.clone());
            let tahap: Vec<TahapOut> = pokmas
                .rpdb
                .get(&norm_nama(&nama))
                .map(|v| {
                    v.iter()
                        .map(|t| TahapOut { jumlah: t.jumlah, tanggal: t.tanggal.map(|d| d.format("%Y-%m-%d").to_string()) })
                        .collect()
                })
                .unwrap_or_default();
            json!({ "jenis": x.jenis(), "nama": nama, "tahap": tahap })
        })
        .collect();
    Ok(Json(json!({ "sheet": format!("M{week}"), "items": items })))
}

#[derive(Deserialize)]
pub struct TahapIn {
    jumlah: Option<i64>,
    tanggal: Option<String>,
}

#[derive(Deserialize)]
pub struct RpdbIn {
    nama: String,
    tahap: Vec<TahapIn>,
}

/// Menyimpan RPDB satu Pokmas (menggantikan semua tahap yang ada).
pub async fn rpdb_simpan(
    State(st): State<AppState>,
    session: Session,
    Json(input): Json<RpdbIn>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let nama = input.nama.trim().to_string();
    if nama.is_empty() || nama.chars().count() > 255 {
        return Err(AppError::BadRequest("Nama Pokmas wajib diisi (maks. 255 karakter)".to_string()));
    }
    if input.tahap.len() > 12 {
        return Err(AppError::BadRequest("Maksimal 12 tahap RPDB".to_string()));
    }
    let mut tahap: Vec<(Option<i64>, Option<NaiveDate>)> = Vec::new();
    for (i, t) in input.tahap.iter().enumerate() {
        if t.jumlah.map_or(false, |j| j < 0) {
            return Err(AppError::BadRequest(format!("Jumlah RPDB {} tidak boleh negatif", i + 1)));
        }
        let tanggal = match t.tanggal.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(s) => Some(
                NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .map_err(|_| AppError::BadRequest(format!("Tanggal RPDB {} tidak valid (pakai YYYY-MM-DD)", i + 1)))?,
            ),
        };
        tahap.push((t.jumlah, tanggal));
    }
    // Tahap kosong di ujung tidak disimpan; tahap kosong di tengah tetap menjaga urutan.
    while tahap.last().map_or(false, |(j, t)| j.is_none() && t.is_none()) {
        tahap.pop();
    }
    let kunci = norm_nama(&nama);
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM lapming_rpdb WHERE nama_norm = ?").bind(&kunci).execute(&mut *tx).await?;
    for (i, (jumlah, tanggal)) in tahap.iter().enumerate() {
        sqlx::query("INSERT INTO lapming_rpdb (nama_norm, nama_baku, tahap_no, jumlah, tanggal) VALUES (?, ?, ?, ?, ?)")
            .bind(&kunci)
            .bind(&nama)
            .bind((i + 1) as u8)
            .bind(jumlah)
            .bind(tanggal)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "tahap": tahap.len() })))
}

#[derive(Serialize, Deserialize)]
pub struct AliasItem {
    sumber: String,
    baku: String,
}

pub async fn alias_daftar(State(st): State<AppState>, session: Session) -> Result<Json<Vec<AliasItem>>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let baris: Vec<(String, String)> = sqlx::query_as("SELECT nama_sumber, nama_baku FROM lapming_alias ORDER BY nama_baku, nama_sumber")
        .fetch_all(&st.db)
        .await?;
    Ok(Json(baris.into_iter().map(|(sumber, baku)| AliasItem { sumber, baku }).collect()))
}

/// Mengganti seluruh daftar alias.
pub async fn alias_simpan(
    State(st): State<AppState>,
    session: Session,
    Json(items): Json<Vec<AliasItem>>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    if items.len() > 500 {
        return Err(AppError::BadRequest("Terlalu banyak alias".to_string()));
    }
    let mut peta: HashMap<String, (String, String)> = HashMap::new();
    for it in &items {
        let (sumber, baku) = (it.sumber.trim(), it.baku.trim());
        if sumber.is_empty() && baku.is_empty() {
            continue;
        }
        if sumber.is_empty() || baku.is_empty() || sumber.chars().count() > 255 || baku.chars().count() > 255 {
            return Err(AppError::BadRequest("Nama sumber dan nama baku wajib diisi (maks. 255 karakter)".to_string()));
        }
        peta.insert(norm_nama(sumber), (sumber.to_string(), baku.to_string()));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM lapming_alias").execute(&mut *tx).await?;
    for (k, (sumber, baku)) in &peta {
        sqlx::query("INSERT INTO lapming_alias (nama_norm, nama_sumber, nama_baku) VALUES (?, ?, ?)")
            .bind(k)
            .bind(sumber)
            .bind(baku)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "alias": peta.len() })))
}
/// Progres Lapming satu pekerjaan: baris minggu dari sumber Sanimas dan LPK yang nama pokmasnya cocok
/// (setelah alias). Dibaca dari database, jadi tidak perlu mengunduh file lagi.
pub async fn proyek(
    State(st): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    crate::projects::ensure_project_access(&st.db, &user, id).await?;
    let pokmas: String = sqlx::query_scalar("SELECT pokmas FROM projects WHERE id = ? AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(&st.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let alias = muat_pokmas(&st.db).await?.alias;
    let efektif = |nama: &str| -> String {
        let k = norm_nama(nama);
        match alias.get(&k) {
            Some(baku) => norm_nama(baku),
            None => k,
        }
    };
    let cari = efektif(&pokmas);

    // Hanya baris yang nama ternormalisasinya bisa menjadi pokmas ini (nama itu sendiri, atau nama sumber alias-nya).
    isi_nama_norm_kosong(&st.db).await?;
    let mut kandidat: Vec<String> = vec![cari.clone()];
    for (k, baku) in &alias {
        if norm_nama(baku) == cari && !kandidat.contains(k) {
            kandidat.push(k.clone());
        }
    }
    let mut qb: QueryBuilder<MySql> = QueryBuilder::new("SELECT CAST(jenis AS CHAR), data FROM lapming_data WHERE nama_norm IN (");
    {
        let mut sep = qb.separated(", ");
        for k in &kandidat {
            sep.push_bind(k.clone());
        }
    }
    qb.push(") ORDER BY id");
    let baris: Vec<(String, String)> = qb.build_query_as().fetch_all(&st.db).await?;
    let mut items: Vec<(u32, Value)> = Vec::new();
    // (minggu, lpk, no, korfas, sheet, nama, ag, ad) untuk pengecekan anomali di bawah.
    let mut cocok: Vec<(u32, bool, String, String, String, String, f64, f64)> = Vec::new();
    for (_, data) in baris {
        let x: Row = serde_json::from_str(&data).map_err(internal)?;
        if efektif(&x.nama) != cari {
            continue;
        }
        let minggu: u32 = x.sheet.trim_start_matches('M').parse().unwrap_or(0);
        let periode = match (x.mulai, x.selesai) {
            (Some(a), Some(b)) => periode(a, b),
            _ => String::new(),
        };
        let tgl = |d: Option<NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string());
        cocok.push((minggu, x.lpk, x.no.clone(), x.korfas.clone(), x.sheet.clone(), x.nama.clone(), x.ag, x.ad));
        items.push((
            minggu,
            json!({
                "jenis": x.jenis(),
                "sheet": x.sheet,
                "minggu": minggu,
                "periode": periode,
                "korfas": x.korfas,
                "tahap": x.tahap,
                "pagu": x.pagu,
                "mulai": tgl(x.mulai),
                "selesai": tgl(x.selesai),
                "pekerja": x.p,
                "hok": x.r,
                "fisik_minggu_ini": x.af,
                "fisik_kumulatif": x.ag,
                "keuangan_minggu_ini": x.ac,
                "keuangan_kumulatif": x.ad,
                "dana_minggu_ini": x.z,
                "dana_kumulatif": x.aa,
                "target": x.ah,
                "deviasi": x.ai(),
            }),
        ));
    }
    // Anomali: "Fisik melebihi keuangan" dari baris itu sendiri, dan temuan cek_kualitas
    // (Fisik turun, tidak menjumlah, di atas 100%, dan lain-lain) yang mengenai Pokmas ini.
    let mut anomali: Vec<(u32, Value)> = Vec::new();
    if !cocok.is_empty() {
        let mut korfas_pilih: Vec<String> = cocok.iter().map(|c| c.3.clone()).collect();
        korfas_pilih.sort();
        korfas_pilih.dedup();
        let ds = muat_dataset_korfas(&st.db, &korfas_pilih).await?;
        let mut sudah: HashMap<(String, u32), (Vec<Row>, Vec<(usize, String, String)>)> = HashMap::new();
        for (minggu, lpk, no, korfas, sheet, nama, ag, ad) in cocok.iter().cloned() {
            let sumber = if lpk { "LPK" } else { "Sanimas" };
            if ag > ad + 0.0005 {
                anomali.push((
                    minggu,
                    json!({
                        "minggu": minggu,
                        "sheet": sheet,
                        "sumber": sumber,
                        "pokmas": nama,
                        "kategori": "Fisik melebihi keuangan",
                        "keterangan": format!("Fisik {:.2}%, keuangan {:.2}%", ag * 100.0, ad * 100.0).replace('.', ","),
                    }),
                ));
            }
            let (rows_m, masalah) = sudah.entry((korfas.clone(), minggu)).or_insert_with(|| {
                let mut rows_m = calc::ambil(&ds, minggu, Some(korfas.as_str()), None);
                calc::kumulatif(&ds, minggu, Some(korfas.as_str()), &mut rows_m, None);
                let masalah = calc::cek_kualitas(&ds, &korfas, minggu, &rows_m, None);
                (rows_m, masalah)
            });
            for (i, kat, ket) in masalah.iter() {
                let r = &rows_m[*i];
                if r.lpk == lpk && r.no == no {
                    anomali.push((
                        minggu,
                        json!({
                            "minggu": minggu,
                            "sheet": sheet,
                            "sumber": sumber,
                            "pokmas": nama,
                            "kategori": kat,
                            "keterangan": ket,
                        }),
                    ));
                }
            }
        }
        anomali.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let anomali: Vec<Value> = anomali.into_iter().map(|(_, v)| v).collect();

    items.sort_by(|a, b| a.0.cmp(&b.0));
    let items: Vec<Value> = items.into_iter().map(|(_, v)| v).collect();
    Ok(Json(json!({ "pokmas": pokmas, "ditemukan": !items.is_empty(), "minggu": items, "anomali": anomali })))
}

#[derive(Deserialize)]
pub struct TrenQuery {
    /// "lpk" untuk LPK. Kosong atau lainnya untuk Sanimas.
    sumber: Option<String>,
    /// Tahap, mis. "Tahap 1". Kosong = semua tahap.
    tahap: Option<String>,
}

/// Tren progres per minggu, dirata-rata tertimbang pagu. Bisa difilter sumber (Sanimas/LPK) dan tahap.
/// Juga mengembalikan daftar tahap yang ada untuk sumber itu, untuk pilihan filter.
pub async fn tren(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<TrenQuery>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let ds = muat_dataset(&st.db).await?;
    let lpk_saja = q.sumber.as_deref() == Some("lpk");
    let tahap_pilih = q.tahap.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let mut tahap_ada: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // minggu -> (jumlah fisik*pagu, jumlah keuangan*pagu, jumlah pagu, jumlah pokmas)
    let mut hitung: BTreeMap<u32, (f64, f64, f64, usize)> = BTreeMap::new();
    for (lpk, b) in ds.books() {
        if lpk != lpk_saja {
            continue;
        }
        for (&w, rows) in &b.sheets {
            for x in rows.iter().filter(|x| x.terisi() && x.pagu > 0.0) {
                let tahap_baris = x.tahap.as_deref().map(str::trim).unwrap_or("");
                if !tahap_baris.is_empty() {
                    tahap_ada.insert(tahap_baris.to_string());
                }
                if let Some(pilih) = tahap_pilih {
                    if tahap_baris != pilih {
                        continue;
                    }
                }
                let e = hitung.entry(w).or_insert((0.0, 0.0, 0.0, 0));
                e.0 += x.ag * x.pagu;
                e.1 += x.ad * x.pagu;
                e.2 += x.pagu;
                e.3 += 1;
            }
        }
    }
    let items: Vec<Value> = hitung
        .into_iter()
        .map(|(w, (fis, keu, pagu, n))| {
            json!({ "minggu": w, "fisik": fis / pagu * 100.0, "keuangan": keu / pagu * 100.0, "pokmas": n })
        })
        .collect();
    let tahap: Vec<String> = tahap_ada.into_iter().collect();
    Ok(Json(json!({ "items": items, "tahap": tahap })))
}

#[derive(Deserialize)]
pub struct MingguQuery {
    /// "lpk" untuk LPK. Kosong atau lainnya untuk Sanimas.
    sumber: Option<String>,
    /// Nomor minggu (M1 = 1). Kosong = minggu terakhir yang ada.
    minggu: Option<u32>,
    tahap: Option<String>,
    korfas: Option<String>,
}

/// Detail progres satu minggu, seperti laporan mingguan: ringkasan, tabel per Pokmas, penggunaan dana,
/// RPDB, dan anomali. Ringkasan dirata-rata tertimbang pagu. Persen dikirim dalam skala 0-100.
pub async fn minggu_detail(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<MingguQuery>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let ds = muat_dataset(&st.db).await?;
    let lpk_saja = q.sumber.as_deref() == Some("lpk");
    let tahap = q.tahap.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let korfas = q.korfas.as_deref().map(str::trim).filter(|t| !t.is_empty());

    let mut tersedia: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut tahap_ada: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut korfas_ada: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (lpk, b) in ds.books() {
        if lpk != lpk_saja {
            continue;
        }
        for (&w, rows) in &b.sheets {
            for x in rows.iter().filter(|x| x.terisi()) {
                tersedia.insert(w);
                if let Some(t) = x.tahap.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    tahap_ada.insert(t.to_string());
                }
                if !x.korfas.trim().is_empty() {
                    korfas_ada.insert(x.korfas.trim().to_string());
                }
            }
        }
    }
    let tersedia_vec: Vec<u32> = tersedia.iter().copied().collect();
    let Some(&terakhir) = tersedia.iter().next_back() else {
        return Ok(Json(json!({ "minggu_tersedia": tersedia_vec, "minggu": null, "ringkasan": null,
                               "baris": [], "tahap": [], "korfas": [], "anomali": [] })));
    };
    let minggu = q.minggu.filter(|m| tersedia.contains(m)).unwrap_or(terakhir);

    let mut baris: Vec<Row> = calc::ambil(&ds, minggu, korfas, tahap)
        .into_iter()
        .filter(|x| x.lpk == lpk_saja && x.terisi())
        .collect();
    calc::kumulatif(&ds, minggu, korfas, &mut baris, tahap);
    baris.sort_by_key(|x| x.no.parse::<u32>().unwrap_or(0));

    // Anomali per Pokmas untuk minggu ini: temuan cek kualitas dan fisik melebihi keuangan.
    let mut anomali_map: HashMap<(bool, String), Vec<String>> = HashMap::new();
    let korfas_baris: std::collections::BTreeSet<String> = baris.iter().map(|x| x.korfas.clone()).collect();
    for k in &korfas_baris {
        let mut rows_k = calc::ambil(&ds, minggu, Some(k.as_str()), tahap);
        calc::kumulatif(&ds, minggu, Some(k.as_str()), &mut rows_k, tahap);
        for (i, kat, _) in calc::cek_kualitas(&ds, k, minggu, &rows_k, tahap) {
            let r = &rows_k[i];
            anomali_map.entry((r.lpk, r.no.clone())).or_default().push(kat);
        }
        for r in rows_k.iter().filter(|r| r.terisi() && r.ag > r.ad + 0.0005) {
            anomali_map.entry((r.lpk, r.no.clone())).or_default().push("Fisik melebihi keuangan".to_string());
        }
    }
    let anomali_total: usize = baris
        .iter()
        .map(|x| anomali_map.get(&(x.lpk, x.no.clone())).map_or(0, |v| v.len()))
        .sum();

    // RPDB per Pokmas (setelah alias). Tahap bertanggal setelah tanggal laporan tidak dihitung.
    let pokmas_data = muat_pokmas(&st.db).await?;
    let tgl_lap = baris.iter().filter_map(|x| x.selesai).max();
    let rpdb_baris: Vec<Option<i64>> = baris
        .iter()
        .map(|x| {
            let k = efektif_nama(&pokmas_data.alias, &x.nama);
            let masuk: Vec<i64> = pokmas_data
                .rpdb
                .get(&k)
                .map(|tahap| {
                    tahap
                        .iter()
                        .filter(|t| t.jumlah.is_some() && (t.tanggal.is_none() || tgl_lap.is_none() || t.tanggal.unwrap() <= tgl_lap.unwrap()))
                        .filter_map(|t| t.jumlah)
                        .collect()
                })
                .unwrap_or_default();
            if masuk.is_empty() { None } else { Some(masuk.iter().sum()) }
        })
        .collect();
    let rpdb_total: i64 = rpdb_baris.iter().flatten().sum();

    let pagu_total: f64 = baris.iter().map(|x| x.pagu).sum();
    let rata = |f: fn(&Row) -> f64| -> f64 {
        if pagu_total > 0.0 {
            baris.iter().map(|x| f(x) * x.pagu).sum::<f64>() / pagu_total * 100.0
        } else {
            0.0
        }
    };
    let (tgt_jumlah, tgt_pagu) = baris
        .iter()
        .filter_map(|x| x.ah.map(|t| (t * x.pagu, x.pagu)))
        .fold((0.0, 0.0), |(a, b), (c, d)| (a + c, b + d));
    let ringkasan = json!({
        "pokmas": baris.len(),
        "pagu_total": pagu_total,
        "fisik_kumulatif": rata(|x| x.ag),
        "keuangan_kumulatif": rata(|x| x.ad),
        "target": if tgt_pagu > 0.0 { json!(tgt_jumlah / tgt_pagu * 100.0) } else { Value::Null },
        "realisasi": baris.iter().map(|x| x.aa).sum::<f64>(),
        "pekerja": baris.iter().map(|x| x.p).sum::<i64>(),
        "hok": baris.iter().map(|x| x.r).sum::<i64>(),
        "rpdb_total": rpdb_total,
        "anomali": anomali_total,
    });
    let mut anomali_daftar: Vec<Value> = Vec::new();
    let daftar: Vec<Value> = baris
        .iter()
        .enumerate()
        .map(|(idx, x)| {
            let tgl = |d: Option<NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string());
            let kat = anomali_map.get(&(x.lpk, x.no.clone())).cloned().unwrap_or_default();
            for k in &kat {
                anomali_daftar.push(json!({ "no": x.no, "pokmas": x.label, "kategori": k }));
            }
            let rpdb = rpdb_baris[idx];
            json!({
                "no": x.no,
                "pokmas": x.label,
                "desa": x.desa,
                "korfas": x.korfas,
                "tahap": x.tahap,
                "mulai": tgl(x.mulai),
                "selesai": tgl(x.selesai),
                "pagu": x.pagu,
                "pekerja": x.p,
                "hok": x.r,
                "fisik_minggu_ini": x.af * 100.0,
                "fisik_kumulatif": x.ag * 100.0,
                "target": x.ah.map(|t| t * 100.0),
                "keuangan_minggu_ini": x.ac * 100.0,
                "keuangan_kumulatif": x.ad * 100.0,
                "dana_minggu_ini": x.z,
                "dana_kumulatif": x.aa,
                "kum_upah": x.b.s,
                "kum_material": x.b.u,
                "kum_bop": x.b.w,
                "rpdb": rpdb,
                "saldo_rpdb": rpdb.map(|r| r as f64 - x.aa),
                "anomali": kat,
            })
        })
        .collect();
    Ok(Json(json!({
        "minggu_tersedia": tersedia_vec,
        "minggu": minggu,
        "ringkasan": ringkasan,
        "baris": daftar,
        "anomali": anomali_daftar,
        "tahap": tahap_ada.into_iter().collect::<Vec<String>>(),
        "korfas": korfas_ada.into_iter().collect::<Vec<String>>(),
    })))
}

/// Pengaturan sumber data: link Sanimas dan LPK, serta tahun anggaran pekerjaan.
pub struct Pengaturan {
    pub link_san: String,
    pub link_lpk: String,
    pub tahun_anggaran: u16,
}

pub async fn baca_pengaturan(db: &MySqlPool) -> Result<Pengaturan, AppError> {
    let baris: Vec<(String, String)> = sqlx::query_as("SELECT kunci, nilai FROM lapming_pengaturan").fetch_all(db).await?;
    let mut out = Pengaturan { link_san: String::new(), link_lpk: String::new(), tahun_anggaran: 2026 };
    for (k, v) in baris {
        match k.as_str() {
            "link_san" => out.link_san = v,
            "link_lpk" => out.link_lpk = v,
            "tahun_anggaran" => out.tahun_anggaran = v.parse().unwrap_or(2026),
            _ => {}
        }
    }
    Ok(out)
}

/// Pengaturan sumber dan 10 update terakhir, untuk dashboard dan halaman Pengaturan.
pub async fn update_status(State(st): State<AppState>, session: Session) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let p = baca_pengaturan(&st.db).await?;
    let riwayat: Vec<(String, String, String, i32, i32, i32, Option<String>, String)> = sqlx::query_as(
        "SELECT DATE_FORMAT(waktu, '%Y-%m-%d %H:%i:%s'), CAST(jenis AS CHAR), status, baris, perubahan, pekerjaan_baru, catatan, pengguna \
         FROM lapming_update_log ORDER BY id DESC LIMIT 10",
    )
    .fetch_all(&st.db)
    .await?;
    let riwayat: Vec<Value> = riwayat
        .into_iter()
        .map(|(waktu, jenis, status, baris, perubahan, pekerjaan, catatan, pengguna)| {
            json!({ "waktu": waktu, "jenis": jenis, "status": status, "baris": baris, "perubahan": perubahan,
                    "pekerjaan_baru": pekerjaan, "catatan": catatan, "pengguna": pengguna })
        })
        .collect();
    Ok(Json(json!({
        "link_san": p.link_san,
        "link_lpk": p.link_lpk,
        "tahun_anggaran": p.tahun_anggaran,
        "bisa_ubah": user.role == "admin",
        "riwayat": riwayat,
    })))
}

#[derive(Deserialize)]
pub struct PengaturanIn {
    link_san: String,
    link_lpk: String,
    tahun_anggaran: u16,
}

/// Menyimpan link sumber dan tahun anggaran. Hanya admin.
pub async fn pengaturan_simpan(
    State(st): State<AppState>,
    session: Session,
    Json(input): Json<PengaturanIn>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    if user.role != "admin" {
        return Err(AppError::NotFound);
    }
    let (san, lpk) = (input.link_san.trim(), input.link_lpk.trim());
    for link in [san, lpk] {
        if !link.contains("google.com/") || link.len() > 1000 {
            return Err(AppError::BadRequest("Link harus berupa link Google Sheets atau Drive".to_string()));
        }
    }
    if !(2000..=2100).contains(&input.tahun_anggaran) {
        return Err(AppError::BadRequest("Tahun tidak valid".to_string()));
    }
    let mut tx = st.db.begin().await?;
    for (k, v) in [
        ("link_san", san.to_string()),
        ("link_lpk", lpk.to_string()),
        ("tahun_anggaran", input.tahun_anggaran.to_string()),
    ] {
        sqlx::query("INSERT INTO lapming_pengaturan (kunci, nilai) VALUES (?, ?) ON DUPLICATE KEY UPDATE nilai = VALUES(nilai)")
            .bind(k)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

fn pesan_galat(e: &AppError) -> String {
    match e {
        AppError::BadRequest(m) | AppError::Conflict(m) => m.clone(),
        AppError::NotFound => "Data tidak ditemukan".to_string(),
        AppError::Unauthorized => "Belum login".to_string(),
        AppError::Internal(_) => "Terjadi kesalahan di server".to_string(),
    }
}

/// Memperbarui data Sanimas dan LPK dari link yang tersimpan, lalu mencatat hasilnya di log.
/// Sanimas juga membuat pekerjaan baru (admin). Satu jenis gagal tidak menghentikan yang lain.
pub async fn perbarui(State(st): State<AppState>, session: Session) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let p = baca_pengaturan(&st.db).await?;
    let mut hasil: Vec<Value> = Vec::new();
    for (jenis, link) in [("san", p.link_san.clone()), ("lpk", p.link_lpk.clone())] {
        let lpk = jenis == "lpk";
        let tahun = if lpk { None } else { Some(p.tahun_anggaran) };
        let coba: Result<Value, AppError> = async {
            let isi = unduh_link(&link).await?;
            let Json(v) = proses_impor(&st, &user, lpk, tahun, Bytes::from(isi)).await?;
            Ok(v)
        }
        .await;
        let (status, baris, perubahan, pekerjaan, catatan) = match coba {
            Ok(v) => (
                "berhasil",
                v["baris"].as_i64().unwrap_or(0),
                v["perubahan"].as_i64().unwrap_or(0),
                v["pekerjaan"]["ditambahkan"].as_i64().unwrap_or(0),
                None,
            ),
            Err(e) => ("gagal", 0, 0, 0, Some(pesan_galat(&e))),
        };
        sqlx::query(
            "INSERT INTO lapming_update_log (pengguna, jenis, status, baris, perubahan, pekerjaan_baru, catatan) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&user.username)
        .bind(jenis)
        .bind(status)
        .bind(baris as i32)
        .bind(perubahan as i32)
        .bind(pekerjaan as i32)
        .bind(&catatan)
        .execute(&st.db)
        .await?;
        hasil.push(json!({ "jenis": jenis, "status": status, "baris": baris, "perubahan": perubahan,
                           "pekerjaan_baru": pekerjaan, "catatan": catatan }));
    }
    Ok(Json(json!({ "hasil": hasil })))
}

#[derive(Deserialize)]
pub struct AnomaliQuery {
    korfas: Option<String>,
}

const ANOMALI_BATAS: usize = 5000;

/// Daftar anomali data dari seluruh minggu, untuk satu korfas (atau semua korfas bila `korfas` kosong).
/// Isinya sama dengan anomali di detail proyek: temuan cek kualitas dan Fisik melebihi keuangan.
pub async fn anomali_daftar(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<AnomaliQuery>,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&session, &st.db).await?;
    butuh_lapming(&user)?;
    let ds = muat_dataset(&st.db).await?;
    // Peta nama Pokmas (setelah alias) ke id pekerjaan, supaya setiap anomali bisa diklik ke detailnya.
    let alias = muat_pokmas(&st.db).await?.alias;
    let mut proyek_id: HashMap<String, u64> = HashMap::new();
    let daftar_proyek: Vec<(u64, String)> =
        sqlx::query_as("SELECT id, pokmas FROM projects WHERE deleted_at IS NULL ORDER BY id").fetch_all(&st.db).await?;
    for (id, pokmas) in daftar_proyek {
        proyek_id.entry(efektif_nama(&alias, &pokmas)).or_insert(id);
    }
    let pilih: Vec<String> = match q.korfas.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(k) => vec![k.to_string()],
        None => calc::daftar_korfas(&ds),
    };
    let mut minggu_ada: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for (_, b) in ds.books() {
        minggu_ada.extend(b.sheets.keys().copied());
    }

    let mut hasil: Vec<(String, u32, Value)> = Vec::new();
    for k in &pilih {
        for &w in &minggu_ada {
            let mut rows = calc::ambil(&ds, w, Some(k.as_str()), None);
            if rows.is_empty() {
                continue;
            }
            // Kumulatif upah, material, BOP harus dihitung dulu; tanpa ini cek "Selisih biaya" salah.
            calc::kumulatif(&ds, w, Some(k.as_str()), &mut rows, None);
            let sumber = |lpk: bool| if lpk { "LPK" } else { "Sanimas" };
            for (i, kat, ket) in calc::cek_kualitas(&ds, k, w, &rows, None) {
                let r = &rows[i];
                hasil.push((
                    r.korfas.clone(),
                    w,
                    json!({
                        "korfas": r.korfas,
                        "minggu": w,
                        "sheet": r.sheet,
                        "sumber": sumber(r.lpk),
                        "pokmas": r.nama,
                        "kategori": kat,
                        "keterangan": ket,
                    }),
                ));
            }
            for r in &rows {
                if r.ag > r.ad + 0.0005 {
                    hasil.push((
                        r.korfas.clone(),
                        w,
                        json!({
                            "korfas": r.korfas,
                            "minggu": w,
                            "sheet": r.sheet,
                            "sumber": sumber(r.lpk),
                            "pokmas": r.nama,
                            "kategori": "Fisik melebihi keuangan",
                            "keterangan": format!("Fisik {:.2}%, keuangan {:.2}%", r.ag * 100.0, r.ad * 100.0).replace('.', ","),
                        }),
                    ));
                }
            }
        }
    }
    hasil.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let total = hasil.len();
    let items: Vec<Value> = hasil
        .into_iter()
        .take(ANOMALI_BATAS)
        .map(|(_, _, mut v)| {
            let nama = v["pokmas"].as_str().unwrap_or("").to_string();
            v["proyek_id"] = json!(proyek_id.get(&efektif_nama(&alias, &nama)).copied());
            v
        })
        .collect();
    Ok(Json(json!({ "total": total, "items": items, "batas": ANOMALI_BATAS })))
}