use anyhow::{Context, Result};
use encoding_rs::Encoding;
use regex::Regex;
use std::fs;
use std::path::Path;

/// Detecta o encoding de um arquivo .srt via análise estatística dos bytes
/// (substitui a chamada externa `file --mime-encoding` do script original).
pub fn detect_encoding(path: &Path) -> Result<&'static Encoding> {
    let bytes = fs::read(path).with_context(|| format!("lendo {:?}", path))?;

    // ASCII/UTF-8 puro: nada a fazer.
    if std::str::from_utf8(&bytes).is_ok() {
        return Ok(encoding_rs::UTF_8);
    }

    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(&bytes, true);
    let enc = detector.guess(None, true);
    Ok(enc)
}

/// Converte o arquivo para UTF-8, usando o encoding detectado.
/// Equivalente a normalize_srt_encoding() no bash (que chamava `iconv`).
pub fn normalize_to_utf8(src: &Path, dst: &Path, enc: &'static Encoding) -> Result<()> {
    if enc == encoding_rs::UTF_8 {
        fs::copy(src, dst)?;
        return Ok(());
    }
    let bytes = fs::read(src)?;
    let (text, _, had_errors) = enc.decode(&bytes);
    fs::write(dst, text.as_bytes())?;
    if had_errors {
        // Mesmo com erros de decodificação parciais, o bash também seguia em frente
        // (fallback silencioso); mantemos o mesmo comportamento permissivo.
    }
    Ok(())
}

fn ts_regex() -> Regex {
    Regex::new(r"(\d{2}):(\d{2}):(\d{2}),(\d{3})").unwrap()
}

fn ts_to_ms(h: i64, m: i64, s: i64, ms: i64) -> i64 {
    h * 3_600_000 + m * 60_000 + s * 1_000 + ms
}

fn ms_to_ts(mut ms: i64) -> String {
    if ms < 0 {
        ms = 0;
    }
    let h = ms / 3_600_000;
    ms -= h * 3_600_000;
    let m = ms / 60_000;
    ms -= m * 60_000;
    let s = ms / 1_000;
    ms -= s * 1_000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, ms)
}

/// Desloca todos os timestamps de um .srt em `shift_sec` segundos (pode ser negativo/decimal).
/// Equivalente a shift_srt() no bash.
pub fn shift_srt(src: &Path, dst: &Path, shift_sec: f64) -> Result<()> {
    let shift_ms = (shift_sec * 1000.0).round() as i64;
    let content = fs::read_to_string(src).with_context(|| format!("lendo {:?}", src))?;
    let re = arrow_regex();

    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let (s1, e1) = (parse_ts(&caps[1]), parse_ts(&caps[2]));
            out.push_str(&format!("{} --> {}\n", ms_to_ts(s1 + shift_ms), ms_to_ts(e1 + shift_ms)));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    fs::write(dst, out)?;
    Ok(())
}

fn arrow_regex() -> Regex {
    Regex::new(r"(\d{2}:\d{2}:\d{2},\d{3}) --> (\d{2}:\d{2}:\d{2},\d{3})").unwrap()
}

fn parse_ts(ts: &str) -> i64 {
    let re = ts_regex();
    let c = re.captures(ts).unwrap();
    ts_to_ms(
        c[1].parse().unwrap(),
        c[2].parse().unwrap(),
        c[3].parse().unwrap(),
        c[4].parse().unwrap(),
    )
}

/// Retorna o primeiro timestamp (em ms) encontrado num .srt, se houver.
pub fn first_timestamp_ms(path: &Path) -> Result<Option<i64>> {
    let content = fs::read_to_string(path).with_context(|| format!("lendo {:?}", path))?;
    let re = ts_regex();
    if let Some(caps) = re.captures(&content) {
        return Ok(Some(ts_to_ms(
            caps[1].parse()?,
            caps[2].parse()?,
            caps[3].parse()?,
            caps[4].parse()?,
        )));
    }
    Ok(None)
}

/// Compara o 1º timestamp do .srt com o start_time do container.
/// Se coincidirem (dentro de 1.5s), é offset de rip -> aplica shift negativo.
/// Caso contrário, a legenda já está sincronizada -> copia sem alterar.
/// Equivalente a auto_sync_srt() no bash.
pub fn auto_sync(src: &Path, dst: &Path, video_start_ms: i64) -> Result<()> {
    const THRESHOLD_MS: i64 = 1500;

    let srt_ms = match first_timestamp_ms(src)? {
        Some(ms) => ms,
        None => {
            println!("⚠️  Sub-sync: nenhum timestamp encontrado no .srt — legenda original mantida");
            fs::copy(src, dst)?;
            return Ok(());
        }
    };

    if srt_ms == 0 {
        println!("🔄 Sub-sync: legenda já começa em 00:00:00 — nenhum ajuste necessário");
        fs::copy(src, dst)?;
        return Ok(());
    }

    let delta = (srt_ms - video_start_ms).abs();

    if delta <= THRESHOLD_MS {
        let offset_sec = srt_ms as f64 / 1000.0;
        println!(
            "🔄 Sub-sync: offset de rip detectado (≈ start_time do vídeo) → deslocando -{:.3}s",
            offset_sec
        );
        shift_srt(src, dst, -offset_sec)?;
    } else {
        println!(
            "🔄 Sub-sync: 1º diálogo em {:.3}s, start_time do vídeo {:.3}s — legenda já sincronizada",
            srt_ms as f64 / 1000.0,
            video_start_ms as f64 / 1000.0
        );
        fs::copy(src, dst)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_shift_positive() {
        let dir = std::env::temp_dir();
        let src = dir.join("t_in.srt");
        let dst = dir.join("t_out.srt");
        fs::write(&src, "1\n00:00:05,000 --> 00:00:07,500\nHello world\n\n2\n00:00:10,200 --> 00:00:12,000\nSecond line\n").unwrap();
        shift_srt(&src, &dst, 2.0).unwrap();
        let out = fs::read_to_string(&dst).unwrap();
        assert!(out.contains("00:00:07,000 --> 00:00:09,500"));
        assert!(out.contains("00:00:12,200 --> 00:00:14,000"));
    }

    #[test]
    fn test_shift_negative_clamped_to_zero() {
        let dir = std::env::temp_dir();
        let src = dir.join("t_in2.srt");
        let dst = dir.join("t_out2.srt");
        fs::write(&src, "1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
        shift_srt(&src, &dst, -5.0).unwrap();
        let out = fs::read_to_string(&dst).unwrap();
        assert!(out.contains("00:00:00,000 --> 00:00:00,000"));
    }

    #[test]
    fn test_first_timestamp() {
        let dir = std::env::temp_dir();
        let src = dir.join("t_in3.srt");
        fs::write(&src, "1\n00:00:05,000 --> 00:00:07,500\nHello\n").unwrap();
        let ms = first_timestamp_ms(&src).unwrap();
        assert_eq!(ms, Some(5000));
    }

    #[test]
    fn test_auto_sync_detects_rip_offset() {
        let dir = std::env::temp_dir();
        let src = dir.join("t_in4.srt");
        let dst = dir.join("t_out4.srt");
        // srt starts at 5.0s, video start_time also ~5.0s -> rip offset, should shift by -5s
        fs::write(&src, "1\n00:00:05,000 --> 00:00:07,500\nHello\n").unwrap();
        auto_sync(&src, &dst, 5000).unwrap();
        let out = fs::read_to_string(&dst).unwrap();
        assert!(out.contains("00:00:00,000 --> 00:00:02,500"));
    }

    #[test]
    fn test_auto_sync_leaves_synced_subtitle() {
        let dir = std::env::temp_dir();
        let src = dir.join("t_in5.srt");
        let dst = dir.join("t_out5.srt");
        // srt starts at 30s dialogue, video start_time 0 -> already synced, no change
        fs::write(&src, "1\n00:00:30,000 --> 00:00:32,000\nHello\n").unwrap();
        auto_sync(&src, &dst, 0).unwrap();
        let out = fs::read_to_string(&dst).unwrap();
        assert!(out.contains("00:00:30,000 --> 00:00:32,000"));
    }
}
