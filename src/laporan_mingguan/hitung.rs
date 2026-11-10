//! Menghitung rumus di workbook hasil dan menyimpan nilainya di file, supaya penampil yang tidak
//! menghitung ulang (pratinjau HP, penampil web) tetap menunjukkan angka. Excel tetap menghitung ulang
//! saat file dibuka.
//!
//! Yang didukung sesuai isi template: angka, persen, teks, + - * /, kurung, rujukan sel dan rentang
//! (termasuk antar sheet), SUM, AVERAGE, COUNTA, IFERROR, dan rumus bersama (shared formula).
//! Rumus di luar itu dibiarkan tanpa nilai tersimpan.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};

use regex::Regex;

use super::isi::{peta_sheet, xml_escape, xml_unescape};

#[derive(Clone, Debug, PartialEq)]
enum Nilai {
    Kosong,
    Angka(f64),
    Teks(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Galat {
    /// #DIV/0!
    BagiNol,
    /// #VALUE!
    Nilai,
    /// Rumus di luar cakupan evaluator: sel dibiarkan tanpa nilai tersimpan.
    TidakDidukung,
}

type Hasil = Result<Nilai, Galat>;
type Kunci = (usize, u32, u32);

struct Sel {
    nilai: Nilai,
    rumus: Option<String>,
}

fn indeks_kolom(huruf: &str) -> u32 {
    huruf.bytes().fold(0, |n, b| n * 26 + (b - b'A' + 1) as u32)
}

fn huruf_kolom(mut c: u32) -> String {
    let mut out = String::new();
    while c > 0 {
        out.insert(0, (b'A' + ((c - 1) % 26) as u8) as char);
        c = (c - 1) / 26;
    }
    out
}

/// Rujukan sel di posisi `i`: (kolom, baris, kolom mutlak, baris mutlak, panjang). None bila bukan rujukan.
fn baca_rujukan(b: &[u8], i: usize) -> Option<(u32, u32, bool, bool, usize)> {
    let mut j = i;
    let kol_mutlak = b.get(j) == Some(&b'$');
    if kol_mutlak {
        j += 1;
    }
    let awal_kol = j;
    while j < b.len() && b[j].is_ascii_uppercase() {
        j += 1;
    }
    if j == awal_kol || j - awal_kol > 3 {
        return None;
    }
    let kol = indeks_kolom(std::str::from_utf8(&b[awal_kol..j]).ok()?);
    let baris_mutlak = b.get(j) == Some(&b'$');
    if baris_mutlak {
        j += 1;
    }
    let awal_baris = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j == awal_baris {
        return None;
    }
    // Diikuti huruf, angka, "(" atau "!" berarti ini nama fungsi atau nama sheet, bukan rujukan.
    if let Some(c) = b.get(j) {
        if c.is_ascii_alphanumeric() || *c == b'_' || *c == b'(' || *c == b'!' || *c == b'.' {
            return None;
        }
    }
    let baris: u32 = std::str::from_utf8(&b[awal_baris..j]).ok()?.parse().ok()?;
    Some((kol, baris, kol_mutlak, baris_mutlak, j - i))
}

/// Menggeser rujukan relatif di rumus bersama sejauh (dk, db). Nama sheet dan teks tidak disentuh.
fn geser_rumus(rumus: &str, dk: i64, db: i64) -> String {
    let b = rumus.as_bytes();
    let mut out = String::with_capacity(rumus.len() + 8);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'\'' || c == b'"' {
            // Salin apa adanya sampai tanda penutup.
            let mut j = i + 1;
            while j < b.len() && b[j] != c {
                j += 1;
            }
            let akhir = (j + 1).min(b.len());
            out.push_str(&rumus[i..akhir]);
            i = akhir;
            continue;
        }
        let batas = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.');
        if batas && (c == b'$' || c.is_ascii_uppercase()) {
            if let Some((kol, baris, km, bm, n)) = baca_rujukan(b, i) {
                let k = if km { kol as i64 } else { kol as i64 + dk };
                let r = if bm { baris as i64 } else { baris as i64 + db };
                if k >= 1 && r >= 1 {
                    if km {
                        out.push('$');
                    }
                    out.push_str(&huruf_kolom(k as u32));
                    if bm {
                        out.push('$');
                    }
                    out.push_str(&r.to_string());
                    i += n;
                    continue;
                }
            }
            // Bukan rujukan (nama fungsi atau nama sheet): salin seluruh kata.
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.' || b[j] == b'$') {
                j += 1;
            }
            let j = j.max(i + 1);
            out.push_str(&rumus[i..j]);
            i = j;
            continue;
        }
        // Karakter non-ASCII disalin utuh.
        let n = rumus[i..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(1);
        out.push_str(&rumus[i..i + n]);
        i += n;
    }
    out
}

/// Argumen fungsi: nilai tunggal atau rentang sel.
enum Arg {
    Nilai(Hasil),
    Rentang(usize, u32, u32, u32, u32),
}

struct Buku {
    nama: HashMap<String, usize>,
    sel: HashMap<Kunci, Sel>,
    memo: HashMap<Kunci, Hasil>,
    sedang: HashSet<Kunci>,
}

struct Pengurai<'a> {
    b: &'a [u8],
    s: &'a str,
    i: usize,
    sheet: usize,
}

impl Buku {
    fn nilai_sel(&mut self, k: Kunci) -> Hasil {
        if let Some(h) = self.memo.get(&k) {
            return h.clone();
        }
        let Some(sel) = self.sel.get(&k) else { return Ok(Nilai::Kosong) };
        let Some(rumus) = sel.rumus.clone() else { return Ok(sel.nilai.clone()) };
        if !self.sedang.insert(k) {
            return Err(Galat::TidakDidukung);
        }
        let mut p = Pengurai { b: rumus.as_bytes(), s: &rumus, i: 0, sheet: k.0 };
        let mut h = p.ekspresi(self);
        p.spasi();
        if h.is_ok() && p.i < p.b.len() {
            h = Err(Galat::TidakDidukung);
        }
        // Rumus yang merujuk sel kosong menghasilkan 0, seperti di Excel.
        if h == Ok(Nilai::Kosong) {
            h = Ok(Nilai::Angka(0.0));
        }
        self.sedang.remove(&k);
        self.memo.insert(k, h.clone());
        h
    }
}

fn angka(n: Nilai) -> Result<f64, Galat> {
    match n {
        Nilai::Kosong => Ok(0.0),
        Nilai::Angka(a) => Ok(a),
        Nilai::Teks(t) => {
            let t = t.trim();
            if t.is_empty() {
                Err(Galat::Nilai)
            } else {
                t.parse::<f64>().map_err(|_| Galat::Nilai)
            }
        }
    }
}

impl Pengurai<'_> {
    fn spasi(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn lihat(&mut self) -> Option<u8> {
        self.spasi();
        self.b.get(self.i).copied()
    }

    fn ekspresi(&mut self, buku: &mut Buku) -> Hasil {
        let mut kiri = self.suku(buku);
        loop {
            match self.lihat() {
                Some(op @ (b'+' | b'-')) => {
                    self.i += 1;
                    let kanan = self.suku(buku);
                    kiri = (|| {
                        let a = angka(kiri.clone()?)?;
                        let b = angka(kanan?)?;
                        Ok(Nilai::Angka(if op == b'+' { a + b } else { a - b }))
                    })();
                }
                _ => return kiri,
            }
        }
    }

    fn suku(&mut self, buku: &mut Buku) -> Hasil {
        let mut kiri = self.faktor(buku);
        loop {
            match self.lihat() {
                Some(op @ (b'*' | b'/')) => {
                    self.i += 1;
                    let kanan = self.faktor(buku);
                    kiri = (|| {
                        let a = angka(kiri.clone()?)?;
                        let b = angka(kanan?)?;
                        if op == b'*' {
                            Ok(Nilai::Angka(a * b))
                        } else if b == 0.0 {
                            Err(Galat::BagiNol)
                        } else {
                            Ok(Nilai::Angka(a / b))
                        }
                    })();
                }
                _ => return kiri,
            }
        }
    }

    fn faktor(&mut self, buku: &mut Buku) -> Hasil {
        match self.lihat() {
            Some(b'-') => {
                self.i += 1;
                let v = self.faktor(buku);
                return v.and_then(|n| Ok(Nilai::Angka(-angka(n)?)));
            }
            Some(b'+') => {
                self.i += 1;
                return self.faktor(buku);
            }
            _ => {}
        }
        let mut v = match self.utama(buku)? {
            Arg::Nilai(h) => h,
            // Rentang hanya sah sebagai argumen fungsi.
            Arg::Rentang(..) => Err(Galat::TidakDidukung),
        };
        while self.lihat() == Some(b'%') {
            self.i += 1;
            v = v.and_then(|n| Ok(Nilai::Angka(angka(n)? / 100.0)));
        }
        v
    }

    /// Unsur dasar. Err di sini berarti rumus tidak bisa diurai; galat hitung dibawa di dalam Arg::Nilai.
    fn utama(&mut self, buku: &mut Buku) -> Result<Arg, Galat> {
        let Some(c) = self.lihat() else { return Err(Galat::TidakDidukung) };
        if c == b'(' {
            self.i += 1;
            let v = self.ekspresi(buku);
            if self.lihat() != Some(b')') {
                return Err(Galat::TidakDidukung);
            }
            self.i += 1;
            return Ok(Arg::Nilai(v));
        }
        if c == b'"' {
            let awal = self.i + 1;
            let mut j = awal;
            while j < self.b.len() && self.b[j] != b'"' {
                j += 1;
            }
            if j >= self.b.len() {
                return Err(Galat::TidakDidukung);
            }
            self.i = j + 1;
            return Ok(Arg::Nilai(Ok(Nilai::Teks(self.s[awal..j].to_string()))));
        }
        if c.is_ascii_digit() || c == b'.' {
            let awal = self.i;
            let mut j = awal;
            while j < self.b.len() && (self.b[j].is_ascii_digit() || self.b[j] == b'.') {
                j += 1;
            }
            // Notasi ilmiah (1E-05) jarang, tetapi sah.
            if j < self.b.len() && (self.b[j] == b'E' || self.b[j] == b'e') {
                let mut k = j + 1;
                if k < self.b.len() && (self.b[k] == b'+' || self.b[k] == b'-') {
                    k += 1;
                }
                if k < self.b.len() && self.b[k].is_ascii_digit() {
                    while k < self.b.len() && self.b[k].is_ascii_digit() {
                        k += 1;
                    }
                    j = k;
                }
            }
            self.i = j;
            let a: f64 = self.s[awal..j].parse().map_err(|_| Galat::TidakDidukung)?;
            return Ok(Arg::Nilai(Ok(Nilai::Angka(a))));
        }

        // Nama sheet (dengan atau tanpa kutip) diikuti "!".
        let mut sheet = self.sheet;
        if c == b'\'' {
            let awal = self.i + 1;
            let mut j = awal;
            while j < self.b.len() && self.b[j] != b'\'' {
                j += 1;
            }
            if self.b.get(j + 1) != Some(&b'!') {
                return Err(Galat::TidakDidukung);
            }
            sheet = *buku.nama.get(&self.s[awal..j]).ok_or(Galat::TidakDidukung)?;
            self.i = j + 2;
        } else {
            let mut j = self.i;
            while j < self.b.len() && (self.b[j].is_ascii_alphanumeric() || self.b[j] == b'_' || self.b[j] == b'.') {
                j += 1;
            }
            if j > self.i && self.b.get(j) == Some(&b'!') {
                sheet = *buku.nama.get(&self.s[self.i..j]).ok_or(Galat::TidakDidukung)?;
                self.i = j + 1;
            } else if j > self.i && self.b.get(j) == Some(&b'(') {
                let nama = self.s[self.i..j].to_ascii_uppercase();
                self.i = j + 1;
                return self.fungsi(buku, &nama).map(Arg::Nilai);
            }
        }

        let (k1, r1, _, _, n) = baca_rujukan(self.b, self.i).ok_or(Galat::TidakDidukung)?;
        self.i += n;
        if self.b.get(self.i) == Some(&b':') {
            let (k2, r2, _, _, n2) = baca_rujukan(self.b, self.i + 1).ok_or(Galat::TidakDidukung)?;
            self.i += 1 + n2;
            return Ok(Arg::Rentang(sheet, k1.min(k2), r1.min(r2), k1.max(k2), r1.max(r2)));
        }
        Ok(Arg::Nilai(buku.nilai_sel((sheet, k1, r1))))
    }

    fn fungsi(&mut self, buku: &mut Buku, nama: &str) -> Result<Hasil, Galat> {
        let mut args: Vec<Arg> = Vec::new();
        if self.lihat() == Some(b')') {
            self.i += 1;
        } else {
            loop {
                // Argumen bisa berupa rentang, jadi coba unsur dasar dulu, lalu ekspresi penuh.
                let simpan = self.i;
                let arg = match self.utama(buku) {
                    Ok(a) if matches!(self.lihat(), Some(b',') | Some(b')')) => a,
                    _ => {
                        self.i = simpan;
                        Arg::Nilai(self.ekspresi(buku))
                    }
                };
                if let Arg::Nilai(Err(Galat::TidakDidukung)) = arg {
                    return Err(Galat::TidakDidukung);
                }
                args.push(arg);
                match self.lihat() {
                    Some(b',') => self.i += 1,
                    Some(b')') => {
                        self.i += 1;
                        break;
                    }
                    _ => return Err(Galat::TidakDidukung),
                }
            }
        }

        // Nilai tiap argumen; isi rentang diurai per sel. `dari_sel` membedakan sel dari nilai langsung.
        let mut nilai: Vec<(Hasil, bool)> = Vec::new();
        for a in args {
            match a {
                Arg::Nilai(h) => nilai.push((h, false)),
                Arg::Rentang(sh, k1, r1, k2, r2) => {
                    for r in r1..=r2 {
                        for k in k1..=k2 {
                            if buku.sel.contains_key(&(sh, k, r)) {
                                nilai.push((buku.nilai_sel((sh, k, r)), true));
                            }
                        }
                    }
                }
            }
        }
        Ok(match nama {
            "IFERROR" => {
                if nilai.len() != 2 {
                    return Err(Galat::TidakDidukung);
                }
                let cadangan = nilai.pop().unwrap().0;
                match nilai.pop().unwrap().0 {
                    Err(Galat::TidakDidukung) => return Err(Galat::TidakDidukung),
                    Err(_) => cadangan,
                    ok => ok,
                }
            }
            "SUM" | "AVERAGE" => {
                let (mut jumlah, mut banyak) = (0.0, 0usize);
                for (h, dari_sel) in nilai {
                    match h {
                        Err(e) => return Ok(Err(e)).and_then(|x: Hasil| if x == Err(Galat::TidakDidukung) { Err(Galat::TidakDidukung) } else { Ok(x) }),
                        Ok(Nilai::Angka(a)) => {
                            jumlah += a;
                            banyak += 1;
                        }
                        // Teks dan sel kosong di dalam rentang diabaikan; teks sebagai argumen langsung adalah galat.
                        Ok(Nilai::Teks(_)) if !dari_sel => return Ok(Err(Galat::Nilai)),
                        Ok(_) => {}
                    }
                }
                if nama == "SUM" {
                    Ok(Nilai::Angka(jumlah))
                } else if banyak == 0 {
                    Err(Galat::BagiNol)
                } else {
                    Ok(Nilai::Angka(jumlah / banyak as f64))
                }
            }
            "COUNTA" => {
                // Semua yang berisi dihitung, termasuk rumus yang menghasilkan teks kosong atau galat.
                let banyak = nilai.iter().filter(|(h, _)| !matches!(h, Ok(Nilai::Kosong))).count();
                if nilai.iter().any(|(h, _)| *h == Err(Galat::TidakDidukung)) {
                    return Err(Galat::TidakDidukung);
                }
                Ok(Nilai::Angka(banyak as f64))
            }
            _ => return Err(Galat::TidakDidukung),
        })
    }
}

/// Menghitung semua rumus yang didukung dan menyimpan nilainya di tiap sel rumus.
pub fn isi_nilai_rumus(xlsx: &[u8]) -> Result<Vec<u8>, String> {
    let mut zin = zip::ZipArchive::new(Cursor::new(xlsx)).map_err(|e| e.to_string())?;
    let mut berkas: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).map_err(|e| e.to_string())?;
        let mut isi = Vec::new();
        f.read_to_end(&mut isi).map_err(|e| e.to_string())?;
        berkas.push((f.name().to_string(), isi));
    }
    let teks = |nama: &str| -> Result<String, String> {
        let (_, isi) = berkas.iter().find(|(n, _)| n == nama).ok_or_else(|| format!("{nama} tidak ada"))?;
        String::from_utf8(isi.clone()).map_err(|e| e.to_string())
    };
    let peta = peta_sheet(&teks("xl/workbook.xml")?, &teks("xl/_rels/workbook.xml.rels")?)?;
    let mut daftar: Vec<(String, String)> = peta.into_iter().collect();
    daftar.sort();

    // Teks bersama (shared strings).
    let re_si = Regex::new(r"(?s)<si>(.*?)</si>").unwrap();
    let re_t = Regex::new(r"(?s)<t[^>]*>(.*?)</t>").unwrap();
    let bersama: Vec<String> = match teks("xl/sharedStrings.xml") {
        Ok(ss) => re_si
            .captures_iter(&ss)
            .map(|c| re_t.captures_iter(&c[1]).map(|t| xml_unescape(&t[1])).collect::<String>())
            .collect(),
        Err(_) => Vec::new(),
    };

    let re_sel = Regex::new(r#"(?s)<c r="([A-Z]+)([0-9]+)"([^>]*?)(?:/>|>(.*?)</c>)"#).unwrap();
    let re_f = Regex::new(r#"(?s)<f([^>]*?)(?:/>|>(.*?)</f>)"#).unwrap();
    let re_v = Regex::new(r"(?s)<v>(.*?)</v>").unwrap();
    let re_tipe = Regex::new(r#" t="([a-zA-Z]+)""#).unwrap();
    let re_si_attr = Regex::new(r#" si="(\d+)""#).unwrap();

    let mut buku = Buku { nama: HashMap::new(), sel: HashMap::new(), memo: HashMap::new(), sedang: HashSet::new() };
    let mut xml_sheet: Vec<String> = Vec::new();
    for (idx, (nama, path)) in daftar.iter().enumerate() {
        buku.nama.insert(nama.clone(), idx);
        let xml = teks(path)?;
        // Rumus induk dari tiap kelompok rumus bersama: si -> (kolom, baris, teks rumus).
        let mut induk: HashMap<String, (u32, u32, String)> = HashMap::new();
        let mut tunggu: Vec<(Kunci, String)> = Vec::new();
        for c in re_sel.captures_iter(&xml) {
            let isi = c.get(4).map(|m| m.as_str()).unwrap_or("");
            if isi.is_empty() {
                continue;
            }
            let kol = indeks_kolom(&c[1]);
            let baris: u32 = c[2].parse().unwrap_or(0);
            let kunci = (idx, kol, baris);
            let tipe = re_tipe.captures(&c[3]).map(|m| m[1].to_string()).unwrap_or_default();
            let nilai = if tipe == "inlineStr" {
                Nilai::Teks(re_t.captures_iter(isi).map(|t| xml_unescape(&t[1])).collect())
            } else {
                match re_v.captures(isi) {
                    None => Nilai::Kosong,
                    Some(v) => match tipe.as_str() {
                        "s" => Nilai::Teks(v[1].parse::<usize>().ok().and_then(|i| bersama.get(i).cloned()).unwrap_or_default()),
                        "str" | "e" => Nilai::Teks(xml_unescape(&v[1])),
                        _ => v[1].trim().parse::<f64>().map(Nilai::Angka).unwrap_or(Nilai::Kosong),
                    },
                }
            };
            let mut rumus = None;
            if let Some(f) = re_f.captures(isi) {
                let attr = f.get(1).map(|m| m.as_str()).unwrap_or("");
                let isi_f = f.get(2).map(|m| xml_unescape(m.as_str())).unwrap_or_default();
                match (attr.contains(r#"t="shared""#), re_si_attr.captures(attr)) {
                    (true, Some(si)) if !isi_f.is_empty() => {
                        induk.insert(si[1].to_string(), (kol, baris, isi_f.clone()));
                        rumus = Some(isi_f);
                    }
                    (true, Some(si)) => tunggu.push((kunci, si[1].to_string())),
                    _ if !isi_f.is_empty() => rumus = Some(isi_f),
                    _ => {}
                }
            }
            if rumus.is_some() || nilai != Nilai::Kosong || re_f.is_match(isi) {
                buku.sel.insert(kunci, Sel { nilai, rumus });
            }
        }
        for (kunci, si) in tunggu {
            if let (Some((k0, r0, teks_rumus)), Some(sel)) = (induk.get(&si), buku.sel.get_mut(&kunci)) {
                sel.rumus = Some(geser_rumus(teks_rumus, kunci.1 as i64 - *k0 as i64, kunci.2 as i64 - *r0 as i64));
            }
        }
        xml_sheet.push(xml);
    }

    // Hitung dan tulis nilai ke setiap sel rumus.
    let kunci_rumus: Vec<Kunci> = buku.sel.iter().filter(|(_, s)| s.rumus.is_some()).map(|(k, _)| *k).collect();
    for k in kunci_rumus {
        buku.nilai_sel(k);
    }
    for (idx, (_, path)) in daftar.iter().enumerate() {
        let xml_baru = re_sel.replace_all(&xml_sheet[idx], |c: &regex::Captures| {
            let utuh = c.get(0).unwrap().as_str();
            let isi = c.get(4).map(|m| m.as_str()).unwrap_or("");
            let Some(f) = re_f.find(isi) else { return utuh.to_string() };
            let kunci = (idx, indeks_kolom(&c[1]), c[2].parse::<u32>().unwrap_or(0));
            let (tipe, nilai) = match buku.memo.get(&kunci) {
                Some(Ok(Nilai::Angka(a))) if a.is_finite() => ("", format!("{a}")),
                Some(Ok(Nilai::Teks(t))) => (r#" t="str""#, xml_escape(t)),
                Some(Err(Galat::BagiNol)) => (r#" t="e""#, "#DIV/0!".to_string()),
                Some(Err(Galat::Nilai)) => (r#" t="e""#, "#VALUE!".to_string()),
                _ => return utuh.to_string(),
            };
            let attr = re_tipe.replace(&c[3], "");
            format!(r#"<c r="{}{}"{attr}{tipe}>{}<v>{nilai}</v></c>"#, &c[1], &c[2], f.as_str())
        });
        if let Some((_, isi)) = berkas.iter_mut().find(|(n, _)| n == path) {
            *isi = xml_baru.into_owned().into_bytes();
        }
    }

    let mut out = Cursor::new(Vec::new());
    {
        let mut zout = zip::ZipWriter::new(&mut out);
        let opsi = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (nama, isi) in &berkas {
            zout.start_file(nama.as_str(), opsi).map_err(|e| e.to_string())?;
            zout.write_all(isi).map_err(|e| e.to_string())?;
        }
        zout.finish().map_err(|e| e.to_string())?;
    }
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rumus_bersama_digeser_tanpa_merusak_nama_sheet() {
        assert_eq!(geser_rumus("'Tahapan TPS3R'!A11", 0, 2), "'Tahapan TPS3R'!A13");
        assert_eq!(geser_rumus("SUM(G13:G62)", 1, 0), "SUM(H13:H62)");
        assert_eq!(geser_rumus("$G11*70%", 2, 3), "$G14*70%");
        assert_eq!(geser_rumus("IFERROR(L11/G$11,\"\")", 0, 1), "IFERROR(L12/G$11,\"\")");
    }
}
