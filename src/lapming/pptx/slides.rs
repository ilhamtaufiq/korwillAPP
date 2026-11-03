//! Susunan slide laporan mingguan (port build() dari generate_lapming.py).

use std::collections::{BTreeSet, HashMap, HashSet};

use super::chart::{ChartSpec, Label, PointLabel, Series};
use super::{Align, Anchor, Deck, Para, Run, Slide, TableOpts, TextOpts};
use crate::lapming::fmt::{num, pct, rb, rp, tgl};
use crate::lapming::log::{ringkas_ubah, LogEntri};
use crate::lapming::model::{Row, Summary};

pub const NAVY: &str = "223468";
pub const KUNING_PU: &str = "FCB717";
pub const KEU_TXT: &str = "8A6200";
pub const BG: &str = "FAF9F6";
pub const TEXT: &str = "1B2540";
pub const MUTED: &str = "5F6B85";
pub const CARD: &str = "EEF1F8";
pub const RULE: &str = "DDE2EE";
pub const MERAH: &str = "B23B2E";
const TARGET: &str = "8B959C";
const LALU: &str = "CDD3E3";
const KERJA: &str = "4F6DB0";
const ALERT_BG: &str = "F8EAE7";
const TINT_FIS: &str = "E3E7F2";
const TINT_KEU: &str = "FFF4D1";

fn kolom_isi(nama: &str) -> String {
    let k = match nama {
        "target kurva S" => "AH",
        "fisik kumulatif" => "AG",
        "keuangan kumulatif" => "AD",
        "dana kumulatif" => "AA",
        "jumlah pekerja" => "P",
        "nama Pokmas" => "G",
        "fisik minggu ini" => "AF",
        "tanggal pelaksanaan" => "N-O",
        _ => return nama.to_string(),
    };
    format!("Kolom {k} ({nama})")
}

fn instruksi(kategori: &str) -> (&'static str, &'static str) {
    match kategori {
        "Di bawah target" => ("AG vs AH", "Cek Fisik kumulatif (AG) dan Target Kurva S (AH); konfirmasi hambatan ke Pokmas/TFL dan susun percepatan"),
        "Fisik melebihi keuangan" => ("AD, AG", "Cocokkan Fisik kumulatif (AG) dengan Keuangan kumulatif (AD); cek pencatatan realisasi dana"),
        "Pekerja 0" => ("P", "Isi Jumlah Pekerja minggu ini (P), atau konfirmasi bila pekerjaan berhenti"),
        "Fisik turun" => ("AE, AF, AG", "Fisik kumulatif tidak boleh turun; periksa AE, AF, AG"),
        "Keuangan turun" => ("AB, AC, AD", "Keuangan kumulatif tidak boleh turun; periksa AB, AC, AD"),
        "Minggu lalu tidak sinkron" => ("AE", "Samakan Fisik minggu lalu (AE) dengan kumulatif sheet minggu sebelumnya"),
        "Fisik tidak menjumlah" => ("AE, AF, AG", "Pastikan Fisik lalu (AE) + ini (AF) = kumulatif (AG)"),
        "Keuangan tidak menjumlah" => ("AB, AC, AD", "Pastikan Keuangan lalu (AB) + ini (AC) = kumulatif (AD)"),
        "Di atas 100%" => ("AG, AD", "Periksa persentase fisik (AG) dan keuangan (AD), maksimum 100%"),
        "Realisasi melebihi pagu" => ("K, Z, AA", "Periksa Pagu (K), Dana minggu ini (Z), dan Dana kumulatif (AA)"),
        "Selisih biaya" => ("S, U, W, AA", "Upah (S) + Material (U) + BOP (W) semua minggu harus sama dengan Dana kumulatif (AA)"),
        "Data kosong" => ("lihat slide berikutnya", "Isi kolom wajib yang kosong, lalu impor ulang file sumber"),
        _ => ("-", "Periksa data pada sheet sumber"),
    }
}

fn content_slide<'a>(deck: &'a mut Deck, judul: &str, footer: &str, sub: Option<&str>) -> &'a mut Slide {
    let nomor = deck.jumlah() + 1;
    let s = deck.tambah(BG);
    s.rect(43.0, 18.0, 43.0, 4.3, KUNING_PU, None);
    s.teks(43.0, 24.0, 871.0, 40.0, judul, &TextOpts::new(28.0).bold().anchor(Anchor::Middle));
    if let Some(t) = sub {
        s.teks(43.0, 64.0, 871.0, 18.0, t, &TextOpts::new(13.0).color(MUTED));
    }
    s.line(43.0, 503.0, 917.0, 503.0, RULE, Some(0.75));
    s.teks(43.0, 509.0, 700.0, 18.0, footer, &TextOpts::new(9.0).color(MUTED));
    s.teks(860.0, 509.0, 57.0, 18.0, &nomor.to_string(), &TextOpts::new(9.0).color(MUTED).align(Align::R));
    s
}

#[allow(clippy::too_many_arguments)]
fn bar_chart(
    cats: Vec<String>,
    series: Vec<(String, Vec<f64>)>,
    colors: &[&str],
    stacked: bool,
    fmt: &str,
    labels: bool,
    title: Option<(String, f64, bool)>,
    value_axis: bool,
    vmin: Option<f64>,
    vmax: Option<f64>,
    size: f64,
) -> ChartSpec {
    let series: Vec<Series> = series
        .into_iter()
        .enumerate()
        .map(|(i, (nm, v))| {
            let mut s = Series::new(&nm, v, colors[i]);
            if labels && !stacked {
                s.label = Some(Label { fmt: fmt.to_string(), size, bold: true, color: TEXT.to_string(), pos: "outEnd" });
            }
            s
        })
        .collect();
    let mut c = ChartSpec::bar(cats, series, size);
    c.stacked = stacked;
    if stacked {
        c.overlap = Some(100);
    }
    c.val_visible = value_axis;
    c.val_min = vmin;
    c.val_max = vmax;
    c.title = title;
    c
}

fn titik_merah(spec: &mut ChartSpec, si: usize, idx: usize, teks: String, size: f64) {
    spec.series[si].pt_colors.push((idx, MERAH.to_string()));
    spec.series[si].pt_labels.push(PointLabel { idx, text: teks, color: MERAH.to_string(), size, bold: true, pos: None });
}

/// Susun data tabel dengan baris pembatas SANIMAS dan LPK; nomor urut mulai dari 1 di tiap kelompok.
fn tabel_grup(
    rows: &[Row],
    head: Vec<&str>,
    buat: impl Fn(usize, &Row) -> Vec<String>,
) -> (Vec<Vec<String>>, HashSet<usize>) {
    let mut data: Vec<Vec<String>> = vec![head.iter().map(|s| s.to_string()).collect()];
    let mut grup = HashSet::new();
    for (nama, lpk) in [("SANIMAS", false), ("LPK", true)] {
        let rs: Vec<&Row> = rows.iter().filter(|x| x.lpk == lpk).collect();
        if rs.is_empty() {
            continue;
        }
        grup.insert(data.len());
        let mut baris = vec![format!("{nama}  ({} Pokmas)", rs.len())];
        baris.extend((1..head.len()).map(|_| String::new()));
        data.push(baris);
        for (i, x) in rs.iter().enumerate() {
            data.push(buat(i + 1, x));
        }
    }
    (data, grup)
}

fn lbl(x: &Row) -> String {
    format!("{}{}", x.label, if x.lpk { " (LPK)" } else { "" })
}

fn pok(x: &Row) -> String {
    format!("Pokmas {}", x.nama)
}

fn rata(rows: &[Row], idx: &[usize], f: impl Fn(&Row) -> f64) -> f64 {
    if idx.is_empty() {
        0.0
    } else {
        idx.iter().map(|&i| f(&rows[i])).sum::<f64>() / idx.len() as f64
    }
}

fn poin(v: f64) -> String {
    format!("{:+.1}", v).replace('.', ",")
}

fn set<T: Copy + Eq + std::hash::Hash>(v: &[T]) -> HashSet<T> {
    v.iter().copied().collect()
}

pub fn build(
    rows: &[Row],
    s: &Summary,
    minggu: u32,
    sheet: &str,
    korfas: &str,
    perubahan: Option<(&[LogEntri], usize)>,
    tahap: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut deck = Deck { judul: format!("Laporan Mingguan Sanimas dan LPK - {korfas} - M{minggu}"), ..Default::default() };
    let per = s.per.clone();
    let footer = format!(
        "Laporan Mingguan Konstruksi Sanimas dan LPK  |  Korfas {korfas}{}  |  Minggu ke-{minggu} ({per})",
        tahap.map(|t| format!("  |  {t}")).unwrap_or_default()
    );
    let (ns, nl) = (s.san.len(), s.lpk.len());
    let semua: Vec<usize> = (0..rows.len()).collect();

    // 1 cover
    {
        let c = deck.tambah(NAVY);
        c.rect(64.8, 118.0, 56.0, 5.0, KUNING_PU, None);
        c.teks(57.6, 136.8, 828.0, 64.8, "Laporan Mingguan Konstruksi", &TextOpts::new(40.0).bold().color("FFFFFF"));
        c.teks(
            57.6,
            205.2,
            828.0,
            43.2,
            &format!("Sanimas dan LPK – Korwil {korfas}, Provinsi Jawa Barat{}", tahap.map(|t| format!(" – {t}")).unwrap_or_default()),
            &TextOpts::new(22.0).color("E4E8F3"),
        );
        c.line(64.8, 282.0, 624.8, 282.0, "4A5B8F", None);
        c.teks(57.6, 295.2, 828.0, 28.8, &format!("Minggu ke-{minggu}  |  Periode pelaksanaan {per}"), &TextOpts::new(16.0).color("FFFFFF"));
        c.teks(
            57.6,
            327.6,
            828.0,
            28.8,
            &format!("{} Pokmas ({ns} Sanimas, {nl} LPK)  |  Sumber: Rekapitulasi Lapming Keuangan dan Fisik, sheet {sheet}", s.n),
            &TextOpts::new(11.0).color("C9D0E3"),
        );
        if !s.kurang.is_empty() {
            c.teks(
                57.6,
                365.0,
                828.0,
                28.8,
                &format!("Catatan: data sheet {sheet} belum lengkap, lihat slide catatan di bagian akhir"),
                &TextOpts::new(13.0).bold().color(KUNING_PU),
            );
        }
    }

    // Summary
    {
        let p_ = |t: String| Para::txt(t).bullet().space(5.0);
        let h_ = |t: String| Para::judul(t, NAVY, 9.0);
        let blok_cap = |nama: &str, g: &[usize]| -> Vec<Para> {
            let mut out = vec![h_(format!("{nama} ({} Pokmas)", g.len()))];
            if g.is_empty() {
                return out;
            }
            let pg: f64 = g.iter().map(|&i| rows[i].pagu).sum();
            let rl: f64 = g.iter().map(|&i| rows[i].aa).sum();
            out.push(p_(format!(
                "Fisik {}, keuangan {} (Rp {} dari pagu Rp {})",
                pct(rata(rows, g, |x| x.ag), 2),
                pct(if pg != 0.0 { rl / pg } else { 0.0 }, 2),
                rb(rl),
                rb(pg)
            )));
            let tg: Vec<usize> = g.iter().copied().filter(|&i| rows[i].ah.is_some()).collect();
            if !tg.is_empty() {
                let gp = (rata(rows, &tg, |x| x.ah.unwrap()) - rata(rows, &tg, |x| x.ag)) * 100.0;
                let mut t = format!(
                    "Target kurva S {}: fisik {} {} poin",
                    pct(rata(rows, &tg, |x| x.ah.unwrap()), 2),
                    if gp > 0.0 { "tertinggal" } else { "unggul" },
                    num(gp.abs(), 1)
                );
                if tg.len() < g.len() {
                    t.push_str(&format!(" (target terisi {} dari {})", tg.len(), g.len()));
                }
                out.push(p_(t));
            } else {
                out.push(p_("Target kurva S belum terisi".to_string()));
            }
            let mut hi = g[0];
            let mut lo = g[0];
            for &i in g {
                if rows[i].ag > rows[hi].ag {
                    hi = i;
                }
                if rows[i].ag < rows[lo].ag {
                    lo = i;
                }
            }
            if g.len() > 1 {
                out.push(p_(format!(
                    "Fisik tertinggi {} {}, terendah {} {}",
                    rows[hi].label,
                    pct(rows[hi].ag, 2),
                    rows[lo].label,
                    pct(rows[lo].ag, 2)
                )));
            }
            out.push(p_(format!(
                "Minggu ini: {} pekerja, {} HOK kumulatif",
                g.iter().map(|&i| rows[i].p).sum::<i64>(),
                rb(g.iter().map(|&i| rows[i].b.r).sum::<f64>())
            )));
            out
        };
        let mut cap = vec![
            h_(format!("Total ({} Pokmas)", s.n)),
            p_(format!("Fisik {}, keuangan {} (Rp {} dari pagu Rp {})", pct(s.fis, 2), pct(s.keu, 2), rb(s.real), rb(s.pagu))),
        ];
        cap.extend(blok_cap("Sanimas", &s.san));
        cap.extend(blok_cap("LPK", &s.lpk));

        let mut tl: Vec<Para> = Vec::new();
        for (nama, g) in [("Sanimas", &s.san), ("LPK", &s.lpk)] {
            if g.is_empty() {
                continue;
            }
            let ng: Vec<usize> = g.iter().copied().filter(|&i| rows[i].ai().map_or(false, |a| a < 0.0)).collect();
            let fk: Vec<usize> = g.iter().copied().filter(|&i| rows[i].ag > rows[i].ad + 0.0005).collect();
            let nl_: Vec<usize> = g.iter().copied().filter(|&i| rows[i].p == 0).collect();
            let mut brs: Vec<Para> = Vec::new();
            if !ng.is_empty() {
                let mut w_ = ng[0];
                for &i in &ng {
                    if rows[i].ai().unwrap() < rows[w_].ai().unwrap() {
                        w_ = i;
                    }
                }
                let n_dev = g.iter().filter(|&&i| rows[i].ai().is_some()).count();
                brs.push(p_(format!(
                    "{} dari {} Pokmas di bawah target kurva S; terbesar {} ({} poin)",
                    ng.len(),
                    n_dev,
                    rows[w_].label,
                    poin(rows[w_].ai().unwrap() * 100.0)
                )));
            }
            if !fk.is_empty() {
                brs.push(p_(format!("Fisik melebihi keuangan: {}", fk.iter().map(|&i| rows[i].label.clone()).collect::<Vec<_>>().join(", "))));
            }
            if !nl_.is_empty() {
                brs.push(p_(format!("Belum ada data pekerja: {}", nl_.iter().map(|&i| rows[i].label.clone()).collect::<Vec<_>>().join(", "))));
            }
            if !brs.is_empty() {
                tl.push(h_(nama.to_string()));
                tl.extend(brs);
            }
        }
        let mut per_kol: Vec<(String, Vec<String>)> = Vec::new();
        for (i, w, _) in &s.kurang {
            for n_ in w {
                match per_kol.iter_mut().find(|(k, _)| k == n_) {
                    Some((_, l)) => l.push(rows[*i].label.clone()),
                    None => per_kol.push((n_.clone(), vec![rows[*i].label.clone()])),
                }
            }
        }
        if !per_kol.is_empty() {
            tl.push(h_("Data yang belum terisi".to_string()));
            for (n_, ls) in &per_kol {
                tl.push(p_(format!("{}: {} Pokmas ({})", kolom_isi(n_), ls.len(), ls.join(", "))));
            }
        }
        if !s.masalah.is_empty() {
            tl.push(h_("Data janggal".to_string()));
            tl.push(p_(format!("{} temuan; lihat sheet Cek Data di xlsx", s.masalah.len())));
        }
        if tl.is_empty() {
            tl.push(p_("Tidak ada temuan yang perlu ditindaklanjuti.".to_string()));
        }
        let sl = content_slide(&mut deck, "Summary", &footer, Some(&format!("Ringkasan kondisi minggu ke-{minggu}, periode {per}")));
        for (x0, ttl, isi, col) in [(43.2, "Capaian", cap, NAVY), (493.2, "Perlu tindak lanjut", tl, KEU_TXT)] {
            sl.rect(x0, 92.0, 421.0, 400.0, CARD, Some(0.03));
            sl.rect(x0, 92.0, 5.0, 400.0, if col == KEU_TXT { KUNING_PU } else { NAVY }, None);
            sl.teks(x0 + 22.0, 102.0, 380.0, 26.0, ttl, &TextOpts::new(18.0).bold().color(col));
            sl.text(x0 + 22.0, 132.0, 385.0, 355.0, isi, &TextOpts::new(11.5));
        }
    }
    // 2 ringkasan
    {
        let jud2 = match s.tgt {
            None => format!("Fisik rata-rata {}; target kurva S belum terisi", pct(s.fis, 2)),
            Some(t) => {
                let gap = (t - s.fis_cmp.unwrap_or(0.0)) * 100.0;
                format!(
                    "Fisik rata-rata {}, {} {} poin dari target kurva S{}",
                    pct(s.fis, 2),
                    if gap > 0.0 { "tertinggal" } else { "unggul" },
                    num(gap.abs(), 1),
                    if s.n_target < s.n { "*" } else { "" }
                )
            }
        };
        let sl = content_slide(&mut deck, "Rekapitulasi", &footer, Some(&jud2));
        let cards = [
            (s.n.to_string(), "Pokmas dipantau", format!("{ns} Sanimas + {nl} LPK")),
            (
                rp(s.pagu_s),
                "Pagu Sanimas",
                if ns > 0 { format!("{ns} Pokmas × Rp {}", rp(rows[s.san[0]].pagu)) } else { String::new() },
            ),
            (
                rp(s.pagu_l),
                "Pagu LPK",
                if nl > 0 { format!("{nl} Pokmas × Rp {}", rp(rows[s.lpk[0]].pagu)) } else { String::new() },
            ),
            (rp(s.real), "Realisasi kumulatif", format!("{} dari pagu", pct(s.keu, 2))),
            (rp(s.real_mgg), "Realisasi minggu ini", format!("{} pekerja · {} HOK", s.pekerja, rb(s.hok as f64))),
        ];
        let (cw, cg) = (164.6, 12.0);
        for (i, (big, lab, sub)) in cards.iter().enumerate() {
            let cx = 43.0 + i as f64 * (cw + cg);
            sl.rect(cx, 108.0, cw, 136.8, CARD, Some(0.06));
            sl.teks(cx + 10.0, 118.8, cw - 20.0, 57.6, big, &TextOpts::new(28.0).bold().anchor(Anchor::Middle));
            sl.teks(cx + 10.0, 176.4, cw - 20.0, 25.2, lab, &TextOpts::new(13.0).bold());
            sl.teks(cx + 10.0, 201.6, cw - 20.0, 25.2, sub, &TextOpts::new(10.5).color(MUTED));
        }
        sl.teks(43.2, 273.6, 432.0, 28.8, "Rata-rata fisik kumulatif vs target kurva S", &TextOpts::new(15.0).bold());
        let mut bars: Vec<(&str, f64, &str)> = vec![("Fisik kumulatif", s.fis, NAVY)];
        if let Some(t) = s.tgt {
            bars.push(("Target kurva S", t, TARGET));
        }
        bars.push(("Keuangan kumulatif (terhadap pagu)", s.keu, KUNING_PU));
        for (k, (lab, val, col)) in bars.iter().enumerate() {
            let yy = 313.2 + k as f64 * 50.4;
            sl.teks(43.2, yy, 237.6, 28.8, lab, &TextOpts::new(13.0).anchor(Anchor::Middle));
            sl.rect(280.8, yy + 5.0, 302.4, 18.7, RULE, Some(0.5));
            if *val > 0.0 {
                sl.rect(280.8, yy + 5.0, (302.4 * val.min(1.0)).max(19.0), 18.7, col, Some(0.5));
            }
            sl.teks(594.0, yy, 64.8, 28.8, &pct(*val, 2), &TextOpts::new(13.0).bold().anchor(Anchor::Middle));
        }
        if s.tgt.is_some() && s.n_target < s.n {
            sl.teks(
                43.2,
                458.0,
                620.0,
                34.0,
                &format!("*Target kurva S baru terisi untuk {} dari {} Pokmas; selisih dan rata-rata target dihitung dari Pokmas tersebut.", s.n_target, s.n),
                &TextOpts::new(9.0).color(MUTED),
            );
        }
        sl.rect(684.0, 273.6, 230.0, 168.0, ALERT_BG, Some(0.06));
        sl.teks(696.0, 280.8, 206.0, 28.8, "Perlu perhatian", &TextOpts::new(15.0).bold().color(MERAH).anchor(Anchor::Middle));
        let nd = s.dev.len();
        let mut pts: Vec<String> = vec![if nd > 0 {
            format!("{} dari {} Pokmas masih di bawah target fisik kurva S", s.neg.len(), nd)
        } else {
            "Target kurva S belum terisi, deviasi belum bisa dihitung".to_string()
        }];
        if let Some(w) = s.worst {
            pts.push(format!("Deviasi terbesar: {}, {}%", rows[w].label, num(rows[w].ai().unwrap() * 100.0, 1).replace('-', "−")));
        }
        if nd < s.n && nd > 0 {
            pts.push(format!("{} Pokmas belum punya target kurva S", s.n - nd));
        }
        if !s.nol.is_empty() {
            pts.push(format!(
                "{}: tidak ada pekerja pada minggu ini",
                s.nol.iter().map(|&i| rows[i].label.clone()).collect::<Vec<_>>().join(", ")
            ));
        } else {
            pts.push(format!("{}: pekerja paling sedikit ({} orang)", rows[s.min_p].label, rows[s.min_p].p));
        }
        let paras: Vec<Para> = pts.iter().take(3).map(|p| Para::txt(p.clone()).bullet().space(4.0)).collect();
        sl.text(696.0, 313.2, 206.0, 124.0, paras, &TextOpts::new(11.5));
    }

    // 3 fisik terurut
    {
        let min_ag = rows.iter().map(|x| x.ag).fold(f64::INFINITY, f64::min);
        let max_ag = rows.iter().map(|x| x.ag).fold(f64::NEG_INFINITY, f64::max);
        let sl = content_slide(
            &mut deck,
            "Rekapitulasi Fisik per Pokmas",
            &footer,
            Some(&format!("Fisik per Pokmas berkisar {} hingga {}", pct(min_ag, 2), pct(max_ag, 2))),
        );
        let blok = [
            (&s.san, (43.2, 92.0, 552.0, 404.0), format!("Sanimas, {ns} Pokmas, Rp {}", rp(s.pagu_s))),
            (&s.lpk, (612.0, 92.0, 302.4, 190.0), format!("LPK, {nl} Pokmas, Rp {}", rp(s.pagu_l))),
        ];
        for (grp, (x0, y0, w0, h0), ttl) in blok {
            if grp.is_empty() {
                continue;
            }
            let mut g = grp.clone();
            g.sort_by(|&a, &b| rows[b].ag.partial_cmp(&rows[a].ag).unwrap());
            let spec = bar_chart(
                g.iter().map(|&i| rows[i].label.clone()).collect(),
                vec![("Fisik kumulatif".to_string(), g.iter().map(|&i| rows[i].ag * 100.0).collect())],
                &[NAVY],
                false,
                "0.00\"%\"",
                true,
                Some((ttl, 13.0, true)),
                false,
                Some(0.0),
                Some(max_ag * 100.0 * 1.25),
                10.0,
            );
            sl.chart(x0, y0, w0, h0, spec);
        }
        sl.rect(612.0, 300.0, 302.4, 196.0, CARD, Some(0.06));
        let mut ringkas: Vec<Para> = vec![Para::runs(vec![Run::new("Rata-rata per kelompok").size(14.0).bold()])];
        for (nm, grp, fis, keu, rl, pg) in [
            ("Sanimas", &s.san, s.fis_s, s.keu_s, s.real_s, s.pagu_s),
            ("LPK", &s.lpk, s.fis_l, s.keu_l, s.real_l, s.pagu_l),
        ] {
            if grp.is_empty() {
                continue;
            }
            ringkas.push(Para::judul(nm, NAVY, 8.0));
            ringkas.push(Para::runs(vec![
                Run::new("Fisik "),
                Run::new(pct(fis, 2)).bold(),
                Run::new("  ·  Keuangan "),
                Run::new(pct(keu, 2)).bold(),
            ]));
            ringkas.push(Para::txt(format!("Realisasi Rp {} dari Rp {}", rp(rl), rp(pg))));
        }
        sl.text(626.0, 310.0, 274.0, 180.0, ringkas, &TextOpts::new(12.0));
    }

    // 4 deviasi
    {
        let dev = &s.dev;
        let tanpa: Vec<usize> = semua.iter().copied().filter(|&i| rows[i].ai().is_none()).collect();
        let jud = if !dev.is_empty() {
            if !s.neg.is_empty() {
                format!("{} dari {} Pokmas masih di bawah target kurva S", s.neg.len(), dev.len())
            } else {
                format!("Semua {} Pokmas di atas target kurva S", dev.len())
            }
        } else {
            "Target kurva S belum terisi, deviasi belum bisa dihitung".to_string()
        };
        let sl = content_slide(&mut deck, "Target dan Realisasi", &footer, Some(&jud));
        sl.teks(
            43.0,
            92.0,
            871.0,
            24.0,
            &format!("Deviasi fisik kumulatif terhadap target kurva S per Pokmas, minggu ke-{minggu} (poin persen)"),
            &TextOpts::new(12.0).color(MUTED),
        );
        let mut ket: Vec<String> = Vec::new();
        if !dev.is_empty() {
            let mut g = dev.clone();
            g.sort_by(|&a, &b| rows[b].ai().unwrap().partial_cmp(&rows[a].ai().unwrap()).unwrap());
            let m = dev.iter().map(|&i| rows[i].ai().unwrap().abs()).fold(0.0, f64::max) * 100.0 + 8.0;
            let mut spec = bar_chart(
                g.iter().map(|&i| lbl(&rows[i])).collect(),
                vec![("Deviasi".to_string(), g.iter().map(|&i| rows[i].ai().unwrap() * 100.0).collect())],
                &[MERAH],
                false,
                "+0.00;\"−\"0.00",
                true,
                None,
                false,
                Some(-m),
                Some(m),
                10.0,
            );
            spec.cat_low = true;
            for (i, &r) in g.iter().enumerate() {
                if rows[r].ai().unwrap() >= 0.0 {
                    spec.series[0].pt_colors.push((i, NAVY.to_string()));
                }
            }
            sl.chart(43.0, 118.0, 871.0, 356.0, spec);
            let ts: Vec<f64> = dev.iter().filter(|&&i| !rows[i].lpk).map(|&i| rows[i].ah.unwrap()).collect();
            let tl: Vec<f64> = dev.iter().filter(|&&i| rows[i].lpk).map(|&i| rows[i].ah.unwrap()).collect();
            let mn = |v: &[f64]| v.iter().cloned().fold(f64::INFINITY, f64::min);
            let mx = |v: &[f64]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if !tl.is_empty() {
                let mut t = format!("Target kurva S LPK minggu ini {}", pct(mn(&tl), 2));
                if mx(&tl) != mn(&tl) {
                    t.push_str(&format!("–{}", pct(mx(&tl), 2)));
                }
                ket.push(t);
            }
            if !ts.is_empty() {
                ket.push(format!("Sanimas {}–{}", pct(mn(&ts), 2), pct(mx(&ts), 2)));
            }
        } else {
            sl.teks(43.0, 200.0, 871.0, 60.0, "Isi kolom target fisik (kurva S) di sheet untuk menampilkan deviasi.", &TextOpts::new(16.0).color(MUTED));
        }
        if !tanpa.is_empty() {
            ket.push(format!("Belum ada target, tidak ditampilkan: {}", tanpa.iter().map(|&i| rows[i].label.clone()).collect::<Vec<_>>().join(", ")));
        }
        let teks = format!("{}{}Sumber: Rekapitulasi Lapming, sheet {sheet}.", ket.join(". "), if ket.is_empty() { "" } else { ". " });
        sl.teks(43.0, 472.0, 871.0, 28.0, &teks, &TextOpts::new(9.0).color(MUTED));
    }

    // 5 chart kolom: fisik vs keuangan kumulatif per Pokmas
    {
        let sel_ = (s.keu - s.fis) * 100.0;
        let sl = content_slide(
            &mut deck,
            "Rata-rata Fisik dan Keuangan",
            &footer,
            Some(&format!("Fisik {} vs keuangan {}, selisih {} poin", pct(s.fis, 2), pct(s.keu, 2), num(sel_.abs(), 1))),
        );
        let mut fis = Series::new("Progres fisik kumulatif (%)", rows.iter().map(|x| x.ag * 100.0).collect(), NAVY);
        fis.label = Some(Label { fmt: "0.00\"%\"".into(), size: 9.0, bold: true, color: NAVY.into(), pos: "outEnd" });
        let mut keu = Series::new("Progres keuangan kumulatif (%)", rows.iter().map(|x| x.ad * 100.0).collect(), KUNING_PU);
        keu.label = Some(Label { fmt: "0.00\"%\"".into(), size: 9.0, bold: true, color: KEU_TXT.into(), pos: "outEnd" });
        let mut spec = ChartSpec::bar(rows.iter().map(lbl).collect(), vec![fis, keu], 9.0);
        spec.col = true;
        spec.gap = 60;
        spec.overlap = Some(-5);
        spec.reverse_cats = false;
        spec.val_visible = true;
        spec.val_min = Some(0.0);
        let mx = rows.iter().map(|x| x.ag.max(x.ad)).fold(0.0, f64::max);
        spec.val_max = Some((100.0 * mx * 1.2).max(10.0));
        spec.val_fmt = Some("0.00\"%\"".into());
        spec.val_size = Some(9.0);
        spec.legend = Some(10.0);
        sl.chart(43.2, 84.0, 873.0, 400.0, spec);
    }

    // 6 tenaga kerja
    {
        let jud = if !s.nol.is_empty() {
            if s.nol.len() == 1 {
                format!("{} tidak punya pekerja minggu ini", rows[s.nol[0]].label)
            } else {
                format!("{} Pokmas tidak punya pekerja minggu ini", s.nol.len())
            }
        } else {
            format!(
                "Pekerja per Pokmas berkisar {} hingga {} orang",
                rows.iter().map(|x| x.p).min().unwrap_or(0),
                rows.iter().map(|x| x.p).max().unwrap_or(0)
            )
        };
        let sl = content_slide(&mut deck, "Jumlah Pekerja Minggu ini dan Kumulatif HOK", &footer, Some(&jud));
        type Kunci = fn(&Row) -> f64;
        let konfig: [(Kunci, &str, &str, f64); 2] = [
            (|x| x.p as f64, "Jumlah pekerja minggu ini (orang)", "0", 43.2),
            (|x| x.b.r, "Kumulatif HOK (hari orang kerja)", "#,##0", 482.4),
        ];
        for (key, ttl, f, x0) in konfig {
            let mut g: Vec<usize> = semua.clone();
            g.sort_by(|&a, &b| key(&rows[b]).partial_cmp(&key(&rows[a])).unwrap());
            let mx = rows.iter().map(|x| key(x)).fold(1.0, f64::max);
            let mut spec = bar_chart(
                g.iter().map(|&i| lbl(&rows[i])).collect(),
                vec![(ttl.to_string(), g.iter().map(|&i| key(&rows[i])).collect())],
                &[KERJA],
                false,
                f,
                true,
                Some((ttl.to_string(), 12.0, false)),
                false,
                Some(0.0),
                Some(mx * 1.15),
                9.0,
            );
            for (i, &r) in g.iter().enumerate() {
                if rows[r].p == 0 {
                    titik_merah(&mut spec, 0, i, format!("{}", key(&rows[r]) as i64), 9.0);
                }
            }
            sl.chart(x0, 90.0, 432.0, 405.0, spec);
        }
    }

    // 7 dana kumulatif per jenis biaya
    let t_u: f64 = rows.iter().map(|x| x.b.s).sum();
    let t_m: f64 = rows.iter().map(|x| x.b.u).sum();
    let t_b: f64 = rows.iter().map(|x| x.b.w).sum();
    {
        let tt = if t_u + t_m + t_b == 0.0 { 1.0 } else { t_u + t_m + t_b };
        let jud = format!(
            "Material {} dari realisasi kumulatif, upah {}, BOP {}",
            pct(t_m / tt, 0),
            pct(t_u / tt, 0),
            pct(t_b / tt, 0)
        );
        let sl = content_slide(&mut deck, "Rekapitulasi Penggunaan Dana", &footer, Some(&jud));
        sl.teks(43.0, 92.0, 500.0, 24.0, "Penggunaan dana kumulatif per jenis biaya per Pokmas, Rp juta", &TextOpts::new(12.0).color(MUTED));
        sl.text(
            560.0,
            94.0,
            354.0,
            18.0,
            vec![Para::runs(vec![
                Run::new("■ ").color(NAVY),
                Run::new("Upah    "),
                Run::new("■ ").color(KUNING_PU),
                Run::new("Material    "),
                Run::new("■ ").color(LALU),
                Run::new("BOP"),
            ])],
            &TextOpts::new(10.0).color(MUTED).align(Align::R),
        );
        let tot = |x: &Row| x.b.s + x.b.u + x.b.w;
        let mut g: Vec<usize> = semua.clone();
        g.sort_by(|&a, &b| tot(&rows[b]).partial_cmp(&tot(&rows[a])).unwrap());
        let mx = rows.iter().map(|x| tot(x)).fold(0.0, f64::max);
        let mut spec = bar_chart(
            g.iter().map(|&i| lbl(&rows[i])).collect(),
            vec![
                ("Upah".to_string(), g.iter().map(|&i| rows[i].b.s / 1e6).collect()),
                ("Material".to_string(), g.iter().map(|&i| rows[i].b.u / 1e6).collect()),
                ("BOP".to_string(), g.iter().map(|&i| rows[i].b.w / 1e6).collect()),
                ("Total".to_string(), g.iter().map(|_| 0.0).collect()),
            ],
            &[NAVY, KUNING_PU, LALU, BG],
            true,
            "",
            false,
            None,
            true,
            Some(0.0),
            Some(mx / 1e6 * 1.2),
            10.0,
        );
        spec.series[3].color = None;
        for (i, &r) in g.iter().enumerate() {
            spec.series[3].pt_labels.push(PointLabel {
                idx: i,
                text: format!("{} jt", rb(tot(&rows[r]) / 1e6)),
                color: TEXT.into(),
                size: 10.0,
                bold: true,
                pos: Some("inBase"),
            });
        }
        sl.chart(43.2, 116.0, 871.2, 382.0, spec);
    }
    // 8 tabel progres
    {
        let jud = if s.keu_dulu == s.n {
            format!("Keuangan kumulatif mendahului fisik di semua {} Pokmas", s.n)
        } else {
            format!("Keuangan kumulatif mendahului fisik di {} dari {} Pokmas", s.keu_dulu, s.n)
        };
        let (data, grup) = tabel_grup(
            rows,
            vec!["No", "Jenis", "Desa", "Nama Pokmas", "Mulai", "Selesai", "Fisik minggu ini", "Fisik kumulatif", "Keuangan minggu ini", "Keuangan kumulatif"],
            |i, x| {
                vec![
                    i.to_string(),
                    x.jenis().to_string(),
                    x.desa.clone(),
                    pok(x),
                    tgl(x.mulai),
                    tgl(x.selesai),
                    pct(x.af, 2),
                    pct(x.ag, 2),
                    pct(x.ac, 2),
                    pct(x.ad, 2),
                ]
            },
        );
        let sl = content_slide(&mut deck, "Kumulatif Fisik dan Keuangan", &footer, Some(&jud));
        sl.table(
            43.2,
            86.4,
            871.2,
            &data,
            &TableOpts {
                widths: &[3.0, 6.0, 10.0, 20.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0],
                num_cols: set(&[0, 4, 5, 6, 7, 8, 9]),
                tint: HashMap::from([(7, (NAVY, TINT_FIS)), (9, (KEU_TXT, TINT_KEU))]),
                size: 10.0,
                row_h: 21.0,
                bold_cols: set(&[7, 9]),
                total: false,
                grup,
                merah: HashSet::new(),
            },
        );
    }

    // 9 tabel dana
    {
        let (mut data, grup) = tabel_grup(
            rows,
            vec!["No", "Nama Pokmas", "Pekerja (orang)", "Hari kerja minggu ini", "HOK minggu ini", "Kum. HOK", "Kum. upah", "Kum. material", "Kum. BOP", "Realisasi kumulatif", "Keuangan kum."],
            |i, x| {
                vec![
                    i.to_string(),
                    pok(x),
                    x.p.to_string(),
                    x.q.to_string(),
                    rb(x.r as f64),
                    rb(x.b.r),
                    rb(x.b.s),
                    rb(x.b.u),
                    rb(x.b.w),
                    rb(x.aa),
                    pct(x.ad, 2),
                ]
            },
        );
        data.push(vec![
            String::new(),
            "Total".into(),
            rows.iter().map(|x| x.p).sum::<i64>().to_string(),
            String::new(),
            rb(s.hok as f64),
            rb(rows.iter().map(|x| x.b.r).sum::<f64>()),
            rb(t_u),
            rb(t_m),
            rb(t_b),
            rb(s.real),
            pct(s.keu, 2),
        ]);
        let mut nc: HashSet<usize> = (2..11).collect();
        nc.insert(0);
        let sl = content_slide(
            &mut deck,
            "Realisasi Kumulatif Keuangan",
            &footer,
            Some(&format!("Realisasi kumulatif Rp {}, {} dari pagu", rp(s.real), pct(s.keu, 2))),
        );
        sl.table(
            43.2,
            86.4,
            871.2,
            &data,
            &TableOpts {
                widths: &[3.0, 17.0, 6.0, 6.0, 6.0, 6.0, 9.0, 11.0, 8.0, 10.0, 7.0],
                num_cols: nc,
                tint: HashMap::from([(10, (KEU_TXT, TINT_KEU))]),
                size: 10.0,
                row_h: 21.0,
                bold_cols: set(&[10]),
                total: true,
                grup,
                merah: HashSet::new(),
            },
        );
    }

    // 10 progres keuangan
    let pakai_rpdb = s.rpdb.is_some();
    let ntahap = rows
        .iter()
        .map(|x| x.rpdb_t.iter().enumerate().filter(|(_, v)| v.is_some()).map(|(i, _)| i + 1).max().unwrap_or(0))
        .max()
        .unwrap_or(0)
        .max(1);
    {
        let sl_judul = format!("Realisasi {} dari pagu Rp {}", pct(s.keu, 2), rp(s.pagu));
        let mut ringkas: Vec<String> = Vec::new();
        if ns > 0 {
            ringkas.push(format!("Sanimas: pagu Rp {}, realisasi Rp {} ({})", rb(s.pagu_s), rb(s.real_s), pct(s.keu_s, 2)));
        }
        if nl > 0 {
            ringkas.push(format!("LPK: pagu Rp {}, realisasi Rp {} ({})", rb(s.pagu_l), rb(s.real_l), pct(s.keu_l, 2)));
        }
        let nm_rp = if ntahap == 1 { "RPDB 1".to_string() } else { "Total RPDB".to_string() };
        let mut head: Vec<String> = ["No", "Jenis", "Nama Pokmas", "Pagu (Rp)", "Realisasi (Rp)", "% Realisasi", "Sisa pagu (Rp)"].iter().map(|x| x.to_string()).collect();
        if pakai_rpdb {
            head.push(format!("{nm_rp} (Rp)"));
            head.push(format!("Saldo {nm_rp} (Rp)"));
        }
        let head_ref: Vec<&str> = head.iter().map(String::as_str).collect();
        let (mut data, grup) = tabel_grup(rows, head_ref, |i, x| {
            let mut r = vec![
                i.to_string(),
                x.jenis().to_string(),
                pok(x),
                rb(x.pagu),
                rb(x.aa),
                pct(if x.pagu != 0.0 { x.aa / x.pagu } else { 0.0 }, 2),
                rb(x.pagu - x.aa),
            ];
            if pakai_rpdb {
                let rp_ = x.rpdb.unwrap_or(0) as f64;
                r.push(rb(rp_));
                r.push(rb(rp_ - x.aa));
            }
            r
        });
        let mut tot = vec![String::new(), String::new(), "Total".to_string(), rb(s.pagu), rb(s.real), pct(s.keu, 2), rb(s.pagu - s.real)];
        if pakai_rpdb {
            let r = s.rpdb.unwrap_or(0) as f64;
            tot.push(rb(r));
            tot.push(rb(r - s.real));
        }
        data.push(tot);
        let mut w: Vec<f64> = vec![3.0, 7.0, 22.0, 12.0, 12.0, 8.0, 12.0];
        if pakai_rpdb {
            w.extend([12.0, 12.0]);
        }
        let mut nc: HashSet<usize> = (3..head.len()).collect();
        nc.insert(0);
        let sl = content_slide(&mut deck, "Progress Keuangan", &footer, Some(&sl_judul));
        sl.teks(43.0, 86.0, 871.0, 20.0, &ringkas.join("   |   "), &TextOpts::new(11.0).color(MUTED));
        sl.table(
            43.2,
            110.0,
            871.2,
            &data,
            &TableOpts {
                widths: &w,
                num_cols: nc,
                tint: HashMap::from([(5, (KEU_TXT, TINT_KEU))]),
                size: 10.0,
                row_h: 20.0,
                bold_cols: set(&[5]),
                total: true,
                grup,
                merah: HashSet::new(),
            },
        );
    }

    // 10b saldo RPDB
    if pakai_rpdb {
        let kt = ntahap;
        let vt = |x: &Row, i: usize| x.rpdb_t.get(i).copied().flatten();
        let rpdb_s = s.rpdb.unwrap_or(0) as f64;
        let sal_neg = rows.iter().filter(|x| x.rpdb.unwrap_or(0) as f64 - x.aa < 0.0).count();
        let tot_sal = rpdb_s - s.real;
        let ket = if kt == 1 { "RPDB 1".to_string() } else { format!("RPDB 1 s/d {kt}") };
        let mut sub = format!("{ket}: Rp {}, realisasi Rp {}, saldo Rp {}", rb(rpdb_s), rb(s.real), rb(tot_sal));
        if sal_neg > 0 {
            sub.push_str(&format!(" - {sal_neg} Pokmas melebihi RPDB"));
        }
        let mut head2: Vec<String> = vec!["No".into(), "Nama Pokmas".into()];
        head2.extend((0..kt).map(|i| format!("RPDB {}", i + 1)));
        if kt > 1 {
            head2.push("Total RPDB".into());
        }
        head2.extend(["Realisasi".into(), "Saldo".into(), "% Serapan".into()]);
        let head_ref: Vec<&str> = head2.iter().map(String::as_str).collect();
        let (mut data2, grup2) = tabel_grup(rows, head_ref, |i, x| {
            let rp_ = x.rpdb.unwrap_or(0) as f64;
            let mut r = vec![i.to_string(), pok(x)];
            for k in 0..kt {
                r.push(vt(x, k).map(|v| rb(v as f64)).unwrap_or_else(|| "-".to_string()));
            }
            if kt > 1 {
                r.push(rb(rp_));
            }
            r.push(rb(x.aa));
            r.push(rb(rp_ - x.aa));
            r.push(pct(if rp_ != 0.0 { x.aa / rp_ } else { 0.0 }, 2));
            r
        });
        let mut tr: Vec<String> = vec![String::new(), "Total".into()];
        for k in 0..kt {
            tr.push(rb(rows.iter().map(|x| vt(x, k).unwrap_or(0) as f64).sum::<f64>()));
        }
        if kt > 1 {
            tr.push(rb(rpdb_s));
        }
        tr.extend([rb(s.real), rb(tot_sal), pct(if rpdb_s != 0.0 { s.real / rpdb_s } else { 0.0 }, 2)]);
        data2.push(tr);
        let ns_ = head2.len();
        let mut w2: Vec<f64> = vec![3.0, 24.0];
        w2.extend(std::iter::repeat(12.0).take(kt));
        if kt > 1 {
            w2.push(13.0);
        }
        w2.extend([13.0, 13.0, 9.0]);
        let sal_col = ns_ - 2;
        let mut merah = HashSet::new();
        for (i, r) in data2.iter().enumerate() {
            if i > 0 && !grup2.contains(&i) && r[sal_col].starts_with('-') {
                merah.insert((i, sal_col));
            }
        }
        let mut nc: HashSet<usize> = (2..ns_).collect();
        nc.insert(0);
        let n_baris = data2.len();
        let sl = content_slide(&mut deck, "Saldo RPDB", &footer, Some(&sub));
        sl.table(
            43.2,
            92.0,
            871.2,
            &data2,
            &TableOpts {
                widths: &w2,
                num_cols: nc,
                tint: HashMap::from([(sal_col, (KEU_TXT, TINT_KEU))]),
                size: 9.0,
                row_h: 16.0,
                bold_cols: set(&[sal_col]),
                total: true,
                grup: grup2,
                merah,
            },
        );
        sl.teks(
            43.0,
            92.0 + 16.0 * n_baris as f64 + 22.0,
            871.0,
            18.0,
            "Saldo = total RPDB yang sudah ditarik - realisasi kumulatif. Tahap bertanggal setelah tanggal laporan tidak dihitung.",
            &TextOpts::new(10.0).color(MUTED),
        );
    }

    // 11 perlu perhatian
    if !s.perhatian.is_empty() {
        let urut = |k: &str| match k {
            "Di bawah target" => 0,
            "Fisik melebihi keuangan" => 1,
            "Pekerja 0" => 2,
            "Data kosong" => 3,
            _ => 4,
        };
        let mut ph = s.perhatian.clone();
        ph.sort_by_key(|t| urut(&t.1));
        let n_pok = ph.iter().map(|t| t.0).collect::<BTreeSet<_>>().len();
        let maks = 12usize;
        let kosong: Vec<&(usize, String, String)> = ph.iter().filter(|t| t.1 == "Data kosong").collect();
        let tampil: Vec<&(usize, String, String)> = ph.iter().filter(|t| t.1 != "Data kosong").collect();
        let mut data: Vec<Vec<String>> = vec![["Pokmas", "Jenis", "Kategori", "Kolom dicek", "Keterangan", "Instruksi"].iter().map(|x| x.to_string()).collect()];
        for t in tampil.iter().take(maks - usize::from(!kosong.is_empty())) {
            let (kol, ins) = instruksi(&t.1);
            data.push(vec![pok(&rows[t.0]), rows[t.0].jenis().to_string(), t.1.clone(), kol.to_string(), t.2.clone(), ins.to_string()]);
        }
        if !kosong.is_empty() {
            let (kol, ins) = instruksi("Data kosong");
            data.push(vec![
                format!("{} Pokmas", kosong.len()),
                String::new(),
                "Data kosong".into(),
                kol.to_string(),
                kosong.iter().map(|t| rows[t.0].nama.clone()).collect::<Vec<_>>().join(", "),
                ins.to_string(),
            ]);
        }
        let mut merah = HashSet::new();
        for (i, r) in data.iter().enumerate() {
            if i > 0 && ["Di bawah target", "Data kosong", "Fisik turun", "Keuangan turun"].contains(&r[2].as_str()) {
                merah.insert((i, 2));
            }
        }
        let total_tampil = tampil.len() + usize::from(!kosong.is_empty());
        let sl = content_slide(
            &mut deck,
            "Anomali Data: Perlu Perhatian",
            &footer,
            Some(&format!("{} temuan di {} Pokmas", ph.len(), n_pok)),
        );
        sl.table(
            43.2,
            86.4,
            871.2,
            &data,
            &TableOpts {
                widths: &[16.0, 6.0, 12.0, 9.0, 24.0, 33.0],
                num_cols: HashSet::new(),
                tint: HashMap::new(),
                size: 9.0,
                row_h: 29.0,
                bold_cols: HashSet::new(),
                total: false,
                grup: HashSet::new(),
                merah,
            },
        );
        if total_tampil > maks {
            sl.teks(
                43.0,
                478.0,
                871.0,
                20.0,
                &format!("{} temuan lain tidak ditampilkan; lihat sheet Cek Data di xlsx.", total_tampil - maks),
                &TextOpts::new(9.0).color(MUTED),
            );
        }
    }

    // catatan kelengkapan data
    if !s.kurang.is_empty() {
        let mut data: Vec<Vec<String>> = vec![["No", "Jenis", "Nama Pokmas", "Lokasi", "Data wajib yang belum terisi", "Data lain yang belum terisi"].iter().map(|x| x.to_string()).collect()];
        for (i, (idx, w, inf)) in s.catatan.iter().enumerate() {
            let x = &rows[*idx];
            let gabung = |v: &[String]| {
                let t = v.iter().map(|n| kolom_isi(n)).collect::<Vec<_>>().join(", ");
                if t.is_empty() {
                    "-".to_string()
                } else {
                    t
                }
            };
            data.push(vec![(i + 1).to_string(), x.jenis().to_string(), pok(x), format!("{sheet} baris {}", x.row), gabung(w), gabung(inf)]);
        }
        let mut merah = HashSet::new();
        for (i, r) in data.iter().enumerate() {
            if i > 0 && r[4] != "-" {
                merah.insert((i, 4));
            }
        }
        let sl = content_slide(
            &mut deck,
            "Anomali Data: Kolom Belum Terisi",
            &footer,
            Some(&format!("Sheet {sheet} belum lengkap: {} Pokmas ada data wajib kosong", s.kurang.len())),
        );
        sl.text(
            43.0,
            84.0,
            871.0,
            52.0,
            vec![
                Para::txt(format!("Instruksi: buka file sumber (Sanimas atau LPK), sheet {sheet}, lalu isi sel kosong pada kolom dan baris yang tercantum di tabel. Setelah itu impor ulang file sumber dan buat laporan lagi agar grafik dan angka rata-rata lengkap.")),
                Para::txt("Sampai terisi: target kurva S kosong dikeluarkan dari rata-rata target dan grafik deviasi; fisik, keuangan, dan dana kumulatif kosong dihitung 0."),
            ],
            &TextOpts::new(11.0).color(TEXT),
        );
        sl.table(
            43.2,
            150.0,
            871.2,
            &data,
            &TableOpts {
                widths: &[3.0, 7.0, 22.0, 13.0, 30.0, 25.0],
                num_cols: set(&[0]),
                tint: HashMap::new(),
                size: 10.0,
                row_h: 21.0,
                bold_cols: HashSet::new(),
                total: false,
                grup: HashSet::new(),
                merah,
            },
        );
    }

    // log perubahan data
    if let Some((entri, total)) = perubahan {
        let per_slide = 10usize;
        let n_tampil = entri.len().min(30);
        let mut hal = 0;
        while hal < n_tampil {
            let potong = &entri[hal..(hal + per_slide).min(entri.len())];
            let judul = if hal == 0 { "Log Perubahan" } else { "Log Perubahan (lanjutan)" };
            let sub = if hal == 0 { Some(format!("{total} perubahan terbaru untuk korfas {korfas}")) } else { None };
            let mut data: Vec<Vec<String>> = vec![["Waktu", "Sumber", "Sheet", "Pokmas", "Perubahan"].iter().map(|x| x.to_string()).collect()];
            for e in potong {
                data.push(vec![
                    e.waktu.clone(),
                    if e.jenis == "lpk" { "LPK".into() } else { "Sanimas".into() },
                    e.sheet.clone(),
                    e.pokmas.clone(),
                    ringkas_ubah(&e.ubah, 3),
                ]);
            }
            let ada_lagi = total > 30 && hal + per_slide >= n_tampil;
            let sl = content_slide(&mut deck, judul, &footer, sub.as_deref());
            if hal == 0 {
                sl.teks(
                    43.0,
                    84.0,
                    871.0,
                    36.0,
                    "Perubahan nilai pada sumber Sanimas dan LPK yang terdeteksi dibanding pemuatan sebelumnya. Waktu adalah saat perubahan terdeteksi.",
                    &TextOpts::new(11.0).color(MUTED),
                );
            }
            sl.table(
                43.2,
                126.0,
                871.2,
                &data,
                &TableOpts {
                    widths: &[11.0, 8.0, 5.0, 18.0, 58.0],
                    num_cols: HashSet::new(),
                    tint: HashMap::new(),
                    size: 9.0,
                    row_h: 30.0,
                    bold_cols: HashSet::new(),
                    total: false,
                    grup: HashSet::new(),
                    merah: HashSet::new(),
                },
            );
            if ada_lagi {
                sl.teks(
                    43.0,
                    478.0,
                    871.0,
                    20.0,
                    &format!("{} perubahan lebih lama tidak ditampilkan; lihat sheet Log Perubahan di xlsx.", total - 30),
                    &TextOpts::new(9.0).color(MUTED),
                );
            }
            hal += per_slide;
        }
    }

    deck.simpan()
}