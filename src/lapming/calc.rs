//! Perhitungan laporan Lapming: pilih sheet, kumulatif, ringkasan, dan temuan data (port dari generate_lapming.py).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Duration, NaiveDate};

use super::fmt::{norm_nama, pct, periode, rb, rp, short};
use super::model::{Cum, Dataset, Row, Summary, Temuan};

/// Data RPDB per tahap untuk satu Pokmas. Tahap yang bertanggal setelah tanggal laporan tidak dihitung.
#[derive(Clone, Debug, Default)]
pub struct TahapRpdb {
    pub jumlah: Option<i64>,
    pub tanggal: Option<NaiveDate>,
}

/// Alias nama Pokmas dan RPDB per Pokmas. Kunci memakai `norm_nama`.
#[derive(Clone, Debug, Default)]
pub struct DataPokmas {
    pub alias: HashMap<String, String>,
    pub rpdb: HashMap<String, Vec<TahapRpdb>>,
}

/// Alias nama bawaan (nama di sumber -> nama baku), sama dengan ALIAS_DEFAULT di skrip Python.
pub fn alias_bawaan() -> HashMap<String, String> {
    [
        ("Tunas Mandiri", "Taruna Mandiri"),
        ("Sauyunan Ciapyung Girang", "Sauyunan Cipayung Girang"),
        ("Karya Muda mandiri", "Karya Muda Mandiri"),
    ]
    .iter()
    .map(|(a, b)| (norm_nama(a), b.to_string()))
    .collect()
}

/// Nomor minggu yang ada di salah satu sumber, urut naik.
pub fn daftar_sheet(ds: &Dataset) -> Vec<u32> {
    let mut s = BTreeSet::new();
    for (_, b) in ds.books() {
        for n in &b.names {
            if let Some(i) = n.strip_prefix('M').and_then(|x| x.parse::<u32>().ok()) {
                s.insert(i);
            }
        }
    }
    s.into_iter().collect()
}

fn cocok(x: &Row, korfas: Option<&str>, tahap: Option<&str>) -> bool {
    if let Some(k) = korfas {
        if x.korfas.trim().to_lowercase() != k.trim().to_lowercase() {
            return false;
        }
    }
    if let Some(t) = tahap {
        if x.tahap.as_deref().unwrap_or("").trim().to_lowercase() != t.trim().to_lowercase() {
            return false;
        }
    }
    true
}

/// Baris sheet minggu `week` dari kedua sumber (Sanimas dulu, lalu LPK), difilter korfas dan tahap.
pub fn ambil(ds: &Dataset, week: u32, korfas: Option<&str>, tahap: Option<&str>) -> Vec<Row> {
    let mut out = Vec::new();
    for (_, b) in ds.books() {
        if let Some(rows) = b.sheets.get(&week) {
            out.extend(rows.iter().filter(|x| cocok(x, korfas, tahap)).cloned());
        }
    }
    out
}

/// Daftar korfas yang ada di semua sheet, urut abjad.
pub fn daftar_korfas(ds: &Dataset) -> Vec<String> {
    let mut m: BTreeMap<String, String> = BTreeMap::new();
    for (_, b) in ds.books() {
        for rows in b.sheets.values() {
            for x in rows {
                let k = x.korfas.trim();
                if !k.is_empty() && k != "0" && k != "None" {
                    m.entry(k.to_lowercase()).or_insert_with(|| k.to_string());
                }
            }
        }
    }
    let mut v: Vec<String> = m.into_values().collect();
    v.sort();
    v
}

/// Nilai Tahap yang ada untuk korfas ini, urut alami (Tahap 1, Tahap 2, ...).
pub fn daftar_tahap(ds: &Dataset, korfas: &str) -> Vec<String> {
    let mut s: BTreeSet<String> = BTreeSet::new();
    for w in daftar_sheet(ds) {
        for x in ambil(ds, w, Some(korfas), None) {
            if let Some(t) = x.tahap {
                s.insert(t);
            }
        }
    }
    let mut v: Vec<String> = s.into_iter().collect();
    v.sort_by_key(|t| {
        let angka: String = t.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        (angka.parse::<u32>().unwrap_or(0), t.clone())
    });
    v
}

/// Jumlahkan HOK, upah, material, BOP dari M1 sampai minggu terpilih (file tidak punya blok kumulatif).
pub fn kumulatif(ds: &Dataset, week: u32, korfas: Option<&str>, rows: &mut [Row], tahap: Option<&str>) {
    let mut acc: HashMap<(bool, String), [f64; 5]> = HashMap::new();
    for (lpk, b) in ds.books() {
        for i in 1..=week {
            let Some(lst) = b.sheets.get(&i) else { continue };
            for x in lst.iter().filter(|x| cocok(x, korfas, tahap)) {
                let a = acc.entry((lpk, x.no.clone())).or_insert([0.0; 5]);
                a[0] += x.q as f64;
                a[1] += x.r as f64;
                a[2] += x.s;
                a[3] += x.u;
                a[4] += x.w;
            }
        }
    }
    for x in rows.iter_mut() {
        let a = acc.get(&(x.lpk, x.no.clone())).copied().unwrap_or([0.0; 5]);
        x.b = Cum { q: a[0], r: a[1], s: a[2], u: a[3], w: a[4], aa: x.aa, z: x.z };
    }
}

/// Periode pelaksanaan yang paling umum di sheet, atau perkiraan dari nomor minggu (dimulai 1 September 2026).
pub fn periode_sheet(rows: &[Row], minggu: u32) -> String {
    let mut hitung: Vec<((NaiveDate, NaiveDate), usize)> = Vec::new();
    for x in rows {
        if let (Some(a), Some(b)) = (x.mulai, x.selesai) {
            match hitung.iter_mut().find(|(k, _)| *k == (a, b)) {
                Some((_, n)) => *n += 1,
                None => hitung.push(((a, b), 1)),
            }
        }
    }
    let mut terbaik: Option<((NaiveDate, NaiveDate), usize)> = None;
    for (k, n) in hitung {
        if terbaik.map_or(true, |(_, m)| n > m) {
            terbaik = Some((k, n));
        }
    }
    if let Some(((a, b), _)) = terbaik {
        return periode(a, b);
    }
    let a = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap() + Duration::days(7 * (minggu as i64 - 1));
    periode(a, a + Duration::days(6))
}

fn rata(rows: &[Row], idx: &[usize], f: impl Fn(&Row) -> f64) -> f64 {
    if idx.is_empty() {
        0.0
    } else {
        idx.iter().map(|&i| f(&rows[i])).sum::<f64>() / idx.len() as f64
    }
}

fn bagi(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        0.0
    } else {
        a / b
    }
}

/// Baris dengan isian kosong: (indeks, wajib, info). Data wajib kosong lebih dulu.
fn catatan(rows: &[Row]) -> Vec<(usize, Vec<String>, Vec<String>)> {
    let mut c: Vec<(usize, Vec<String>, Vec<String>)> = rows
        .iter()
        .enumerate()
        .filter(|(_, x)| !x.wajib.is_empty() || !x.info.is_empty())
        .map(|(i, x)| (i, x.wajib.clone(), x.info.clone()))
        .collect();
    c.sort_by_key(|t| t.1.is_empty());
    c
}

pub fn hitung(rows: &[Row], minggu: u32) -> Summary {
    let all: Vec<usize> = (0..rows.len()).collect();
    let san: Vec<usize> = all.iter().copied().filter(|&i| !rows[i].lpk).collect();
    let lpk: Vec<usize> = all.iter().copied().filter(|&i| rows[i].lpk).collect();
    let jumlah = |idx: &[usize], f: &dyn Fn(&Row) -> f64| idx.iter().map(|&i| f(&rows[i])).sum::<f64>();
    let ada_t: Vec<usize> = all.iter().copied().filter(|&i| rows[i].ah.is_some()).collect();
    let dev: Vec<usize> = all.iter().copied().filter(|&i| rows[i].ai().is_some()).collect();

    let pagu = jumlah(&all, &|x| x.pagu);
    let pagu_s = jumlah(&san, &|x| x.pagu);
    let pagu_l = jumlah(&lpk, &|x| x.pagu);
    let real = jumlah(&all, &|x| x.aa);
    let real_s = jumlah(&san, &|x| x.aa);
    let real_l = jumlah(&lpk, &|x| x.aa);
    let neg: Vec<usize> = dev.iter().copied().filter(|&i| rows[i].ai().unwrap() < 0.0).collect();
    let mut worst: Option<usize> = None;
    for &i in &dev {
        if worst.map_or(true, |w| rows[i].ai().unwrap() < rows[w].ai().unwrap()) {
            worst = Some(i);
        }
    }
    let mut min_p = 0;
    for i in 0..rows.len() {
        if rows[i].p < rows[min_p].p {
            min_p = i;
        }
    }
    let cat = catatan(rows);
    let kurang: Vec<_> = cat.iter().filter(|c| !c.1.is_empty()).cloned().collect();

    Summary {
        n: rows.len(),
        pagu_s,
        pagu_l,
        pagu,
        real_s,
        real_l,
        real,
        real_mgg: jumlah(&all, &|x| x.z),
        pekerja: rows.iter().map(|x| x.p).sum(),
        hok: rows.iter().map(|x| x.r).sum(),
        fis: rata(rows, &all, |x| x.ag),
        fis_s: rata(rows, &san, |x| x.ag),
        fis_l: rata(rows, &lpk, |x| x.ag),
        naik: rata(rows, &all, |x| x.af),
        n_target: ada_t.len(),
        tgt: if ada_t.is_empty() { None } else { Some(rata(rows, &ada_t, |x| x.ah.unwrap())) },
        fis_cmp: if ada_t.is_empty() { None } else { Some(rata(rows, &ada_t, |x| x.ag)) },
        per: periode_sheet(rows, minggu),
        keu: bagi(real, pagu),
        keu_s: if san.is_empty() { 0.0 } else { bagi(real_s, pagu_s) },
        keu_l: if lpk.is_empty() { 0.0 } else { bagi(real_l, pagu_l) },
        neg,
        worst,
        nol: all.iter().copied().filter(|&i| rows[i].p == 0).collect(),
        min_p,
        keu_dulu: rows.iter().filter(|x| x.ad > x.ag).count(),
        catatan: cat,
        kurang,
        dev,
        san,
        lpk,
        ..Default::default()
    }
}

/// Memilih sheet (atau minggu terakhir yang sudah terisi), membaca baris, dan menghitung ringkasan.
/// Sheet yang belum lengkap tetap diproses; kekurangannya ada di `Summary.catatan`.
pub fn siapkan(
    ds: &Dataset,
    korfas: &str,
    pilih: Option<u32>,
    tahap: Option<&str>,
) -> Result<(u32, Vec<Row>, Summary), String> {
    let names = daftar_sheet(ds);
    if let Some(p) = pilih {
        if !names.contains(&p) {
            let ada: Vec<String> = names.iter().map(|n| format!("M{n}")).collect();
            return Err(format!("Sheet M{p} tidak ada. Tersedia: {}", ada.join(", ")));
        }
    }
    let ket_tahap = tahap.map(|t| format!(" ({t})")).unwrap_or_default();
    let week = match pilih {
        Some(p) => p,
        None => *names
            .iter()
            .rev()
            .find(|&&n| ambil(ds, n, Some(korfas), tahap).iter().any(|x| x.terisi()))
            .ok_or_else(|| format!("Belum ada sheet yang terisi untuk korfas {korfas}{ket_tahap}."))?,
    };
    let mut rows = ambil(ds, week, Some(korfas), tahap);
    if rows.is_empty() {
        let kt = tahap.map(|t| format!(" tahap {t}")).unwrap_or_default();
        return Err(format!("Tidak ada Pokmas untuk korfas {korfas}{kt} di sheet M{week}."));
    }
    kumulatif(ds, week, Some(korfas), &mut rows, tahap);
    let s = hitung(&rows, week);
    Ok((week, rows, s))
}

/// Kumulatif upah+material+BOP dari M1..Mn seharusnya sama dengan dana kumulatif di sheet.
pub fn cek_selisih(rows: &[Row]) -> Vec<usize> {
    (0..rows.len())
        .filter(|&i| (rows[i].b.s + rows[i].b.u + rows[i].b.w - rows[i].aa).abs() > 1000.0)
        .collect()
}

fn pc(a: f64) -> String {
    format!("{:.1}%", a * 100.0).replace('.', ",")
}

/// Temuan data janggal: (indeks, kategori, keterangan).
pub fn cek_kualitas(ds: &Dataset, korfas: &str, week: u32, rows: &[Row], tahap: Option<&str>) -> Vec<Temuan> {
    let mut lalu: HashMap<(bool, String), Row> = HashMap::new();
    if week > 1 {
        for x in ambil(ds, week - 1, Some(korfas), tahap) {
            if x.terisi() {
                lalu.insert((x.lpk, x.no.clone()), x);
            }
        }
    }
    let n = week;
    let mut out: Vec<Temuan> = Vec::new();
    for (i, x) in rows.iter().enumerate() {
        if !x.terisi() {
            continue;
        }
        if let Some(p) = lalu.get(&(x.lpk, x.no.clone())) {
            if x.ag < p.ag - 0.0005 {
                out.push((i, "Fisik turun".into(), format!("Fisik kum. {} (M{}) menjadi {}", pc(p.ag), n - 1, pc(x.ag))));
            }
            if x.ad < p.ad - 0.0005 {
                out.push((i, "Keuangan turun".into(), format!("Keuangan kum. {} (M{}) menjadi {}", pc(p.ad), n - 1, pc(x.ad))));
            }
            if (x.ae - p.ag).abs() > 0.002 {
                out.push((
                    i,
                    "Minggu lalu tidak sinkron".into(),
                    format!("Fisik minggu lalu {} tidak sama dengan kum. M{} {}", pc(x.ae), n - 1, pc(p.ag)),
                ));
            }
        }
        if (x.ae + x.af - x.ag).abs() > 0.002 {
            out.push((
                i,
                "Fisik tidak menjumlah".into(),
                format!("Lalu {} + ini {} tidak sama dengan kum. {}", pc(x.ae), pc(x.af), pc(x.ag)),
            ));
        }
        if (x.ab + x.ac - x.ad).abs() > 0.002 {
            out.push((
                i,
                "Keuangan tidak menjumlah".into(),
                format!("Lalu {} + ini {} tidak sama dengan kum. {}", pc(x.ab), pc(x.ac), pc(x.ad)),
            ));
        }
        if x.ag > 1.0005 || x.ad > 1.0005 {
            out.push((i, "Di atas 100%".into(), format!("Fisik {}, keuangan {}", pc(x.ag), pc(x.ad))));
        }
        if x.pagu != 0.0 && x.aa > x.pagu + 1000.0 {
            out.push((
                i,
                "Realisasi melebihi pagu".into(),
                format!("Realisasi Rp {} dari pagu Rp {}", rb(x.aa), rb(x.pagu)),
            ));
        }
    }
    for i in cek_selisih(rows) {
        let x = &rows[i];
        out.push((
            i,
            "Selisih biaya".into(),
            format!(
                "Upah+material+BOP M1..M{} Rp {} tidak sama dengan dana kumulatif Rp {}",
                n,
                rb(x.b.s + x.b.u + x.b.w),
                rb(x.aa)
            ),
        ));
    }
    out
}

/// Daftar temuan untuk slide Perlu perhatian, urut prioritas.
pub fn perhatian(rows: &[Row], s: &Summary, masalah: &[Temuan]) -> Vec<Temuan> {
    let mut out: Vec<Temuan> = Vec::new();
    let mut bawah: Vec<usize> = s.dev.iter().copied().filter(|&i| rows[i].ai().unwrap() < -0.05).collect();
    bawah.sort_by(|&a, &b| rows[a].ai().unwrap().partial_cmp(&rows[b].ai().unwrap()).unwrap());
    for i in bawah.into_iter().take(6) {
        let x = &rows[i];
        let ai = x.ai().unwrap();
        out.push((
            i,
            "Di bawah target".into(),
            format!(
                "Fisik {} vs target {} ({} poin)",
                pc(x.ag),
                pc(x.ah.unwrap()),
                format!("{:+.1}", ai * 100.0).replace('.', ",")
            ),
        ));
    }
    for (i, x) in rows.iter().enumerate() {
        if x.ag > x.ad + 0.0005 {
            out.push((i, "Fisik melebihi keuangan".into(), format!("Fisik {}, keuangan {}", pc(x.ag), pc(x.ad))));
        }
    }
    for &i in &s.nol {
        if rows[i].terisi() {
            out.push((i, "Pekerja 0".into(), "Jumlah pekerja minggu ini 0".into()));
        }
    }
    out.extend(masalah.iter().cloned());
    for (i, w, _) in &s.kurang {
        out.push((*i, "Data kosong".into(), format!("{} kosong", w.join(", "))));
    }
    out
}

/// Menyamakan nama Pokmas dengan nama baku dan mengisi RPDB. Tahap dihitung bila tanggalnya kosong atau
/// tidak melewati tanggal laporan. Mengembalikan pesan untuk log.
pub fn terapkan_pokmas(rows: &mut [Row], data: &DataPokmas, tgl_laporan: Option<NaiveDate>) -> Vec<String> {
    for x in rows.iter_mut() {
        let mut k = norm_nama(&x.nama);
        if let Some(baru) = data.alias.get(&k) {
            x.label = short(&format!("Pokmas {baru}"));
            x.nama = baru.clone();
            k = norm_nama(&x.nama);
        }
        let tahap = data.rpdb.get(&k).cloned().unwrap_or_default();
        let rt: Vec<Option<i64>> = tahap
            .iter()
            .map(|t| {
                let ok = t.jumlah.is_some()
                    && (t.tanggal.is_none() || tgl_laporan.is_none() || t.tanggal.unwrap() <= tgl_laporan.unwrap());
                if ok {
                    t.jumlah
                } else {
                    None
                }
            })
            .collect();
        x.rpdb = if rt.iter().any(|v| v.is_some()) { Some(rt.iter().flatten().sum()) } else { None };
        x.rpdb_t = rt;
    }
    let kosong: Vec<String> = rows.iter().filter(|x| x.rpdb.is_none()).map(|x| x.nama.clone()).collect();
    let mut log = Vec::new();
    if !kosong.is_empty() && !data.rpdb.is_empty() {
        log.push(format!("RPDB belum diisi (dianggap Rp 0, belum ditarik) untuk: {}", kosong.join(", ")));
        for x in rows.iter_mut() {
            if x.rpdb.is_none() {
                x.rpdb_t = vec![None];
                x.rpdb = Some(0);
            }
        }
    }
    log
}

/// Hasil lengkap satu laporan: baris yang sudah memakai nama baku dan RPDB, serta ringkasannya.
pub struct Laporan {
    pub week: u32,
    pub rows: Vec<Row>,
    pub s: Summary,
    pub log: Vec<String>,
}

/// Seluruh perhitungan untuk satu laporan (proses() di skrip Python, tanpa penulisan file).
pub fn susun(
    ds: &Dataset,
    korfas: &str,
    pilih: Option<u32>,
    tahap: Option<&str>,
    pokmas: &DataPokmas,
) -> Result<Laporan, String> {
    let (week, mut rows, mut s) = siapkan(ds, korfas, pilih, tahap)?;
    let tgl_lap = rows.iter().filter_map(|x| x.selesai).max();
    let log = terapkan_pokmas(&mut rows, pokmas, tgl_lap);
    s.rpdb = if !rows.is_empty() && rows.iter().all(|x| x.rpdb.is_some()) {
        Some(rows.iter().map(|x| x.rpdb.unwrap()).sum())
    } else {
        None
    };
    s.masalah = cek_kualitas(ds, korfas, week, &rows, tahap);
    s.perhatian = perhatian(&rows, &s, &s.masalah);
    Ok(Laporan { week, rows, s, log })
}

/// Ringkasan sebelum membuat laporan (tanpa menulis file).
pub fn pratinjau(ds: &Dataset, korfas: &str, pilih: Option<u32>, tahap: Option<&str>) -> Result<Vec<String>, String> {
    let (sh, rows, s) = siapkan(ds, korfas, pilih, tahap)?;
    let masalah = cek_kualitas(ds, korfas, sh, &rows, tahap);
    let ph = perhatian(&rows, &s, &masalah);
    let n_pok = ph.iter().map(|t| t.0).collect::<BTreeSet<_>>().len();
    let mut t = vec![
        format!("Minggu terdeteksi: M{} (periode {})", sh, s.per),
        format!("Pokmas: {} ({} Sanimas, {} LPK)", s.n, s.san.len(), s.lpk.len()),
        format!(
            "Fisik {}{}, realisasi Rp {} ({} dari pagu)",
            pct(s.fis, 2),
            s.tgt.map(|g| format!(" vs target {}", pct(g, 2))).unwrap_or_default(),
            rp(s.real),
            pct(s.keu, 2)
        ),
        format!("Data wajib kosong: {} Pokmas", s.kurang.len()),
        format!("Data janggal: {} temuan", masalah.len()),
        format!("Perlu perhatian: {} temuan di {} Pokmas", ph.len(), n_pok),
    ];
    for (i, k, ket) in masalah.iter().take(15) {
        t.push(format!("  - {}: {}: {}", rows[*i].label, k, ket));
    }
    Ok(t)
}