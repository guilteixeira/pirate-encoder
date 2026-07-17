use crate::ffmpeg;
use crate::tmdb::TmdbMeta;
use anyhow::Result;
use regex::Regex;
use std::path::{Path, PathBuf};

/// Organiza o arquivo de saída em output/Título (Ano)/... ou output/Título/Season XX/...
/// Equivalente a organize_output() no bash.
pub fn organize_output(
    file: &Path,
    src_name: &str,
    tmdb: Option<&TmdbMeta>,
    embed_meta: bool,
) -> Result<PathBuf> {
    let se_re = Regex::new(r"[Ss](\d{2})[Ee]\d{2}").unwrap();
    let year_re = Regex::new(r"(19\d{2}|20\d{2})").unwrap();

    let season_num = se_re.captures(src_name).map(|c| c[1].to_string());
    let year = year_re.captures(src_name).map(|c| c[1].to_string());

    let parsed = src_name.rsplit_once('.').map(|(a, _)| a).unwrap_or(src_name).replace('.', " ");
    let fallback_title = if let Some(s) = &season_num {
        let full_re = Regex::new(&format!(r"[Ss]{}[Ee]\d{{2}}.*", s)).unwrap();
        full_re.replace(&parsed, "").trim().to_string()
    } else if let Some(y) = &year {
        parsed.split(y.as_str()).next().unwrap_or(&parsed).trim().to_string()
    } else {
        let junk_re = Regex::new(
            r"(?i) (PROPER|REPACK|EXTENDED|2160p|1080p|720p|BluRay|WEB-DL|REMUX|HEVC|x265|x264|DTS|AAC|HDR|DV|Hybrid).*",
        )
        .unwrap();
        junk_re.replace(&parsed, "").trim().to_string()
    };

    let final_title = tmdb.and_then(|t| t.title.clone()).unwrap_or(fallback_title);
    let final_year = tmdb.and_then(|t| t.year.clone()).or(year);

    let safe_title = final_title
        .chars()
        .filter(|c| !r#"/:*?"<>|\"#.contains(*c))
        .collect::<String>();
    let safe_title = Regex::new(r"\s+").unwrap().replace_all(safe_title.trim(), " ").to_string();

    let (dest_dir, dest_file) = if let Some(s) = &season_num {
        println!("📁 Série → {}/Season {}/", safe_title, s);
        (
            PathBuf::from(format!("output/{}/Season {}", safe_title, s)),
            file.file_name().unwrap().to_string_lossy().to_string(),
        )
    } else {
        let folder = match &final_year {
            Some(y) => format!("{} ({})", safe_title, y),
            None => safe_title.clone(),
        };
        println!("📁 Filme → {}/", folder);
        (PathBuf::from(format!("output/{}", folder)), format!("{}.mkv", folder))
    };

    std::fs::create_dir_all(&dest_dir)?;
    let dest_path = dest_dir.join(&dest_file);

    let has_meta = tmdb.map(|t| !t.to_metadata_args().is_empty()).unwrap_or(false);
    if embed_meta && has_meta {
        println!("📝 Embutindo metadados TMDB...");
        let meta_args = tmdb.unwrap().to_metadata_args();
        let ok = ffmpeg::run_remux(file, &[], &["-map".into(), "0".into()], &meta_args, &dest_path)?;
        if ok {
            if file != dest_path {
                let _ = std::fs::remove_file(file);
            }
        } else {
            println!("⚠️  ffmpeg falhou ao embutir metadados — movendo sem metadados");
            std::fs::rename(file, &dest_path)?;
        }
    } else {
        std::fs::rename(file, &dest_path)?;
    }

    println!("   ✓ {}", dest_path.display());
    Ok(dest_path)
}
