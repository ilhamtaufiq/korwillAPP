//! Log perubahan data antar pemuatan: bandingkan nilai penting tiap Pokmas dengan pemuatan sebelumnya.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::fmt::{g, pct, rb};
use super::model::{Dataset, Row, TRACK};

/// Nilai yang dilacak: angka, tanggal (YYYY-MM-DD), atau kosong.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Val {
    Num(f64),
    Tgl(String),
}

pub type Nilai = Option<Val>;

#[derive(Clone, Debug)]
pub struct SnapRow {
    pub label: String,
    pub korfas: String,
    pub terisi: bool,
    pub v: BTreeMap<String, Nilai>,
}

/// Satu perubahan nilai: kolom, nilai lama, nilai baru. Kolom "baris" berarti baris ditambah atau dihapus.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ubah {
    pub f: String,
    pub lama: Nilai,
    pub baru: Nilai,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntri {
    pub waktu: String,
    pub jenis: String,
    pub sheet: String,
    pub no: String,
    pub pokmas: String,
    pub korfas: String,
    pub ubah: Vec<Ubah>,
}

fn nilai_baris(x: &Row, f: &str) -> Nilai {
    let angka = |v: f64| Some(Val::Num((v * 1e6).round() / 1e6));
    match f {
        "mulai" => x.mulai.map(|d| Val::Tgl(d.format("%Y-%m-%d").to_string())),
        "selesai" => x.selesai.map(|d| Val::Tgl(d.format("%Y-%m-%d").to_string())),
        "P" => angka(x.p as f64),
        "Q" => angka(x.q as f64),
        "R" => angka(x.r as f64),
        "S" => angka(x.s),
        "U" => angka(x.u),
        "W" => angka(x.w),
        "Z" => angka(x.z),
        "AA" => angka(x.aa),
        "AC" => angka(x.ac),
        "AD" => angka(x.ad),
        "AF" => angka(x.af),
        "AG" => angka(x.ag),
        "AH" => x.ah.and_then(angka),
        "AI" => x.ai().and_then(angka),
        _ => None,
    }
}

pub fn kunci(jenis_lpk: bool, sheet: &str, no: &str) -> String {
    format!("{}|{}|{}", if jenis_lpk { "lpk" } else { "san" }, sheet, no)
}

/// Rekam nilai penting semua Pokmas (semua korfas) di semua sheet minggu.
pub fn snapshot(ds: &Dataset) -> BTreeMap<String, SnapRow> {
    let mut snap = BTreeMap::new();
    for (lpk, b) in ds.books() {
        for rows in b.sheets.values() {
            for x in rows {
                let v = TRACK.iter().map(|f| (f.to_string(), nilai_baris(x, f))).collect();
                snap.insert(
                    kunci(lpk, &x.sheet, &x.no),
                    SnapRow { label: x.label.clone(), korfas: x.korfas.clone(), terisi: x.terisi(), v },
                );
            }
        }
    }
    snap
}

fn kosong(v: &Nilai) -> bool {
    match v {
        None => true,
        Some(Val::Num(n)) => *n == 0.0,
        Some(Val::Tgl(_)) => false,
    }
}

fn sama(a: &Nilai, b: &Nilai) -> bool {
    if kosong(a) && kosong(b) {
        return true;
    }
    match (a, b) {
        (Some(Val::Num(x)), Some(Val::Num(y))) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

/// Bandingkan dua snapshot; hanya sheet yang sudah terisi (menghindari gema rumus ke sheet kosong).
pub fn bandingkan(lama: &BTreeMap<String, SnapRow>, baru: &BTreeMap<String, SnapRow>, waktu: &str) -> Vec<LogEntri> {
    let pecah = |k: &str| -> (String, String, String) {
        let mut it = k.splitn(3, '|');
        (
            it.next().unwrap_or("").to_string(),
            it.next().unwrap_or("").to_string(),
            it.next().unwrap_or("").to_string(),
        )
    };
    let mut aktif: BTreeSet<(String, String)> = BTreeSet::new();
    for d in [lama, baru] {
        for (k, v) in d {
            if v.terisi {
                let (j, s, _) = pecah(k);
                aktif.insert((j, s));
            }
        }
    }
    let mut out = Vec::new();
    for (k, b) in baru {
        let (jenis, sheet, no) = pecah(k);
        if !aktif.contains(&(jenis.clone(), sheet.clone())) {
            continue;
        }
        let ubah: Vec<Ubah> = match lama.get(k) {
            None => vec![Ubah { f: "baris".into(), lama: None, baru: Some(Val::Tgl("ditambahkan".into())) }],
            Some(a) => TRACK
                .iter()
                .filter(|f| !sama(&a.v[**f], &b.v[**f]))
                .map(|f| Ubah { f: f.to_string(), lama: a.v[*f].clone(), baru: b.v[*f].clone() })
                .collect(),
        };
        if !ubah.is_empty() {
            out.push(LogEntri { waktu: waktu.into(), jenis, sheet, no, pokmas: b.label.clone(), korfas: b.korfas.clone(), ubah });
        }
    }
    for (k, a) in lama {
        let (jenis, sheet, no) = pecah(k);
        if !baru.contains_key(k) && aktif.contains(&(jenis.clone(), sheet.clone())) {
            out.push(LogEntri {
                waktu: waktu.into(),
                jenis,
                sheet,
                no,
                pokmas: a.label.clone(),
                korfas: a.korfas.clone(),
                ubah: vec![Ubah { f: "baris".into(), lama: None, baru: Some(Val::Tgl("dihapus".into())) }],
            });
        }
    }
    out
}

/// Nilai untuk tampilan: "kosong", tanggal dd/mm, persen, rupiah, atau angka biasa.
pub fn fmt_val(f: &str, v: &Nilai) -> String {
    let Some(v) = v else { return "kosong".to_string() };
    match (f, v) {
        ("mulai" | "selesai", Val::Tgl(s)) if s.len() >= 10 => format!("{}/{}", &s[8..10], &s[5..7]),
        ("AC" | "AD" | "AF" | "AG" | "AH" | "AI", Val::Num(n)) => pct(*n, 2),
        ("Z" | "AA" | "S" | "U" | "W", Val::Num(n)) => rb(*n),
        (_, Val::Num(n)) => g(*n),
        (_, Val::Tgl(s)) => s.clone(),
    }
}

/// Ringkasan beberapa perubahan dalam satu kalimat, mis. "Fisik kum. 40,00% → 45,00%".
pub fn ringkas_ubah(ubah: &[Ubah], maks: usize) -> String {
    let mut t = Vec::new();
    for u in ubah.iter().take(maks) {
        if u.f == "baris" {
            let b = match &u.baru {
                Some(Val::Tgl(s)) => s.clone(),
                _ => String::new(),
            };
            t.push(format!("baris {b}"));
        } else {
            t.push(format!("{} {} → {}", super::model::field_nama(&u.f), fmt_val(&u.f, &u.lama), fmt_val(&u.f, &u.baru)));
        }
    }
    let s = t.join("; ");
    if ubah.len() > maks {
        format!("{s} (+{} lainnya)", ubah.len() - maks)
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baris(no: &str, ag: f64, p: i64) -> Row {
        Row { no: no.into(), sheet: "M1".into(), label: "A".into(), ag, p, ..Default::default() }
    }

    #[test]
    fn perubahan_terdeteksi() {
        let mut a = Dataset::default();
        a.san = Some(super::super::model::Book {
            names: vec!["M1".into()],
            sheets: [(1, vec![baris("1", 0.4, 5)])].into_iter().collect(),
        });
        let mut b = a.clone();
        b.san.as_mut().unwrap().sheets.get_mut(&1).unwrap()[0].ag = 0.45;
        let e = bandingkan(&snapshot(&a), &snapshot(&b), "2026-11-03 10:00:00");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].ubah[0].f, "AG");
        assert_eq!(ringkas_ubah(&e[0].ubah, 3), "Fisik kum. 40,00% → 45,00%");
    }
}