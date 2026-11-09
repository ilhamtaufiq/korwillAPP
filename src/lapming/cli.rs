//! Perintah `lapming-cli` untuk mencoba generator dari terminal tanpa database.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::{calc, model::Dataset, read, xlsx};

/// Pemakaian: lapming-cli --sanimas a.xlsx --lpk b.xlsx --korfas Ganthi [--sheet M9] [--tahap "Tahap 1"] [--out folder] [--form]
/// `--form` ikut menulis Form Laporan Mingguan (template resmi) ke folder `--out`, lewat jalur yang sama dengan server.
pub fn jalankan(args: &[String]) -> anyhow::Result<()> {
    let mut san = None;
    let mut lpk = None;
    let mut korfas = "Ganthi".to_string();
    let mut sheet = None;
    let mut tahap = None;
    let mut out = None;
    let mut form = false;
    let mut i = 0;
    while i < args.len() {
        let nilai = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--sanimas" => san = nilai,
            "--lpk" => lpk = nilai,
            "--korfas" => korfas = nilai.unwrap_or(korfas),
            "--sheet" => sheet = nilai,
            "--tahap" => tahap = nilai,
            "--out" => out = nilai,
            "--form" => {
                form = true;
                i += 1;
                continue;
            }
            _ => {}
        }
        i += 2;
    }
    let mut ds = Dataset::default();
    if let Some(p) = san {
        ds.san = Some(read::baca_book(&fs::read(&p)?, false).map_err(|e| anyhow::anyhow!(e))?);
    }
    if let Some(p) = lpk {
        ds.lpk = Some(read::baca_book(&fs::read(&p)?, true).map_err(|e| anyhow::anyhow!(e))?);
    }
    let pilih = sheet.as_deref().and_then(|s| s.trim_start_matches('M').parse::<u32>().ok());
    for baris in calc::pratinjau(&ds, &korfas, pilih, tahap.as_deref()).map_err(|e| anyhow::anyhow!(e))? {
        println!("{baris}");
    }
    if let Some(folder) = out {
        let pokmas = calc::DataPokmas { alias: calc::alias_bawaan(), ..Default::default() };
        let lap = calc::susun(&ds, &korfas, pilih, tahap.as_deref(), &pokmas)
            .map_err(|e| anyhow::anyhow!(e))?;
        fs::create_dir_all(&folder)?;
        let bytes = xlsx::tulis_gabungan(&ds, &lap, &korfas, tahap.as_deref(), &pokmas.alias, &[])?;
        let p = Path::new(&folder).join(format!("Lapming Konstruksi Sanimas dan LPK - {korfas}.xlsx"));
        fs::write(&p, bytes)?;
        println!("xlsx: {}", p.display());
        let sh = format!("M{}", lap.week);
        let pptx = super::pptx::slides::build(&lap.rows, &lap.s, lap.week, &sh, &korfas, None, tahap.as_deref())
            .map_err(|e| anyhow::anyhow!(e))?;
        let pp = Path::new(&folder).join(format!("Lapming Sanimas dan LPK - {korfas} - {sh}.pptx"));
        fs::write(&pp, pptx)?;
        println!("pptx: {}", pp.display());
        if form {
            let (isi, minggu) = crate::laporan_mingguan::bangun_dari_dataset(
                &ds,
                &korfas,
                pilih,
                &pokmas,
                &crate::laporan_mingguan::store::FormData::default(),
            )
            .map_err(|e| anyhow::anyhow!(e))?;
            let pf = Path::new(&folder).join(format!("Form Laporan Mingguan - {korfas} - M{minggu}.xlsx"));
            fs::write(&pf, isi)?;
            println!("form: {}", pf.display());
        }
    }
    Ok(())
}