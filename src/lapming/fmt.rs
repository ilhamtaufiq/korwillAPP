//! Format angka, tanggal, dan nama seperti di generate_lapming.py (gaya Indonesia).

use chrono::{Datelike, NaiveDate};
use regex::Regex;

pub const BULAN: [&str; 13] = [
    "", "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober",
    "November", "Desember",
];

/// Menyisipkan pemisah ribuan (titik) pada bagian bulat string angka "-1234567".
fn ribuan(int_part: &str) -> String {
    let (neg, digits) = match int_part.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, int_part),
    };
    let mut out = String::new();
    let n = digits.len();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    if neg {
        format!("-{out}")
    } else {
        out
    }
}

/// Persen dari pecahan: 0.5343 -> "53,43%".
pub fn pct(v: f64, d: usize) -> String {
    format!("{:.*}", d, v * 100.0).replace('.', ",") + "%"
}

/// Angka dengan ribuan titik dan desimal koma: 1234.5 -> "1.234,5".
pub fn num(v: f64, d: usize) -> String {
    let s = format!("{:.*}", d, v);
    let (int_part, frac) = match s.split_once('.') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (s, None),
    };
    let mut out = ribuan(&int_part);
    if let Some(f) = frac {
        out.push(',');
        out.push_str(&f);
    }
    out
}

/// Ribuan titik tanpa desimal: 3584212700 -> "3.584.212.700".
pub fn rb(v: f64) -> String {
    ribuan(&format!("{:.0}", v))
}

/// Rupiah ringkas: jutaan ("350 jt") atau miliar ("3,58 M").
pub fn rp(v: f64) -> String {
    let jt = v / 1e6;
    if jt >= 1000.0 {
        let m = jt / 1000.0;
        let satu = (m * 10.0 - (m * 10.0).round()).abs() < 1e-9;
        let s = if satu { format!("{:.1}", m) } else { format!("{:.2}", m) };
        return s.replace('.', ",") + " M";
    }
    ribuan(&format!("{:.0}", jt)) + " jt"
}

pub fn tgl(d: Option<NaiveDate>) -> String {
    d.map(|d| d.format("%d/%m/%Y").to_string()).unwrap_or_default()
}

/// "26 Oktober – 1 November 2026" atau "1–7 September 2026".
pub fn periode(a: NaiveDate, b: NaiveDate) -> String {
    if a.month() == b.month() {
        return format!("{}–{} {} {}", a.day(), b.day(), BULAN[b.month() as usize], b.year());
    }
    format!(
        "{} {} – {} {} {}",
        a.day(),
        BULAN[a.month() as usize],
        b.day(),
        BULAN[b.month() as usize],
        b.year()
    )
}

/// Nama pendek untuk label: tanpa kata "Pokmas", "mandiri" jadi "Mandiri".
pub fn short(nama: &str) -> String {
    let n = nama.replace("Pokmas ", "");
    let n = n.trim();
    let re = Regex::new(r"\bmandiri\b").unwrap();
    let mut n = re.replace_all(n, "Mandiri").to_string();
    if n.starts_with("Berkah Jaya Mandiri") {
        n = "Berkah Jaya Mandiri".to_string();
    }
    n
}

/// Nama tanpa awalan "Pokmas" (tidak peduli huruf besar), spasi diratakan.
pub fn tanpa_pokmas(nama: &str) -> String {
    let re = Regex::new(r"(?i)^\s*pokmas\s+").unwrap();
    re.replace(nama, "").to_string()
}

/// Kunci pencocokan nama: huruf kecil, tanpa awalan Pokmas, hanya a-z dan 0-9.
pub fn norm_nama(n: &str) -> String {
    tanpa_pokmas(n)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Titik desimal Python `f"{x:g}"` secukupnya untuk nilai log (bilangan bulat tanpa ".0").
pub fn g(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{}", v);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pct_dan_num() {
        assert_eq!(pct(0.5343, 2), "53,43%");
        assert_eq!(pct(0.0, 2), "0,00%");
        assert_eq!(num(2.7, 1), "2,7");
        assert_eq!(num(1234.5, 1), "1.234,5");
        assert_eq!(num(-9.1, 1), "-9,1");
    }

    #[test]
    fn rupiah() {
        assert_eq!(rb(3584212700.0), "3.584.212.700");
        assert_eq!(rp(350_000_000.0), "350 jt");
        assert_eq!(rp(3_584_212_700.0), "3,58 M");
        assert_eq!(rp(6_100_000_000.0), "6,1 M");
    }

    #[test]
    fn nama() {
        assert_eq!(short("Pokmas Taruna mandiri"), "Taruna Mandiri");
        assert_eq!(short("Pokmas Berkah Jaya Mandiri Sukawijaya"), "Berkah Jaya Mandiri");
        assert_eq!(norm_nama("Pokmas Karya Muda mandiri"), "karyamudamandiri");
    }

    #[test]
    fn periode_bulan() {
        let a = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        let b = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(periode(a, b), "26 Oktober – 1 November 2026");
        let c = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let d = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        assert_eq!(periode(c, d), "1–7 September 2026");
    }
}
