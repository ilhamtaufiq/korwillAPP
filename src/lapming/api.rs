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

#[derive(Deserialize)]
pub struct ImporQuery {
    jenis: String,
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
    proses_impor(&st, lpk, body).await
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
    proses_impor(&st, lpk, Bytes::from(isi)).await
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

async fn proses_impor(st: &AppState, lpk: bool, body: Bytes) -> Result<Json<Value>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("File kosong".to_string()));
    }
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
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new("INSERT INTO lapming_data (jenis, sheet, no_key, korfas, data) ");
        let mut json_baris = Vec::with_capacity(potong.len());
        for x in potong {
            json_baris.push(serde_json::to_string(x).map_err(internal)?);
        }
        qb.push_values(potong.iter().zip(json_baris.iter()), |mut b, (x, j)| {
            b.push_bind(jenis).push_bind(&x.sheet).push_bind(&x.no).push_bind(&x.korfas).push_bind(j);
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

    Ok(Json(json!({
        "sheet_dibaca": book.sheets.len(),
        "sheet_total": book.names.len(),
        "baris": semua.len(),
        "pertama_kali": !punya_lama,
        "perubahan": entri.len(),
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

    let baris: Vec<(String, String)> = sqlx::query_as("SELECT CAST(jenis AS CHAR), data FROM lapming_data ORDER BY id")
        .fetch_all(&st.db)
        .await?;
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
        let ds = muat_dataset(&st.db).await?;
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
                let rows_m = calc::ambil(&ds, minggu, Some(korfas.as_str()), None);
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
            let rows = calc::ambil(&ds, w, Some(k.as_str()), None);
            if rows.is_empty() {
                continue;
            }
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
    let items: Vec<Value> = hasil.into_iter().take(ANOMALI_BATAS).map(|(_, _, v)| v).collect();
    Ok(Json(json!({ "total": total, "items": items, "batas": ANOMALI_BATAS })))
}