mod cli;
mod ffmpeg;
mod organize;
mod platform;
mod subtitle;
mod tmdb;

use anyhow::{Context, Result};
use clap::Parser;
use std::io::Write;
use std::path::{Path, PathBuf};
use tmdb::TmdbMeta;

fn expand_files(patterns: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for pat in patterns {
        if pat.contains('*') || pat.contains('?') {
            if let Ok(paths) = glob::glob(pat) {
                for p in paths.flatten() {
                    if p.is_file() {
                        files.push(p);
                    }
                }
            }
        } else {
            let p = PathBuf::from(pat);
            if p.is_file() {
                files.push(p);
            }
        }
    }
    files
}

fn clean_name(input: &Path) -> String {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    stem.replace('.', " ")
}

fn build_audio_map(audio_lang: &Option<String>, audio_idx: &Option<usize>) -> Vec<String> {
    if let Some(langs) = audio_lang {
        langs
            .split(',')
            .flat_map(|l| vec!["-map".to_string(), format!("0:a:m:language:{}", l)])
            .collect()
    } else if let Some(idx) = audio_idx {
        vec!["-map".into(), format!("0:a:{}", idx)]
    } else {
        vec!["-map".into(), "0:a".into()]
    }
}

fn main() -> Result<()> {
    let args = cli::Args::parse();

    if args.files.is_empty() {
        anyhow::bail!("Nenhum arquivo informado");
    }

    let tune = cli::resolve_tune(&args.tune);
    let files = expand_files(&args.files);

    if files.is_empty() {
        eprintln!("❌ Nenhum arquivo encontrado");
        std::process::exit(1);
    }

    println!("✅ Encontrados {} arquivo(s) para processar", files.len());
    if args.organize_only {
        println!("📂 Modo: organize-only (sem encode)");
    } else if args.sub_only {
        println!("📋 Modo: sub-only (remux, sem re-encode)");
    } else {
        println!("📋 Preset: {} @ {}", tune.name, tune.bitrate);
    }
    if let Some(al) = &args.audio_lang {
        println!("🔊 Áudio: lang={} (ordem preservada)", al);
    } else if let Some(ai) = &args.audio_index {
        println!("🔊 Áudio: somente índice {} (0-based)", ai);
    }
    if let Some(sl) = &args.sub_lang {
        println!("💬 Legenda embarcada: preservando lang={}", sl);
    }
    if args.title_from_file {
        println!("🏷️  Título: será reescrito pelo nome do arquivo");
    } else if args.title_eng {
        println!("🏷️  Título: será extraído da tag existente (parte após ' / ')");
    }
    if let Some(shift) = args.sub_shift {
        println!("⏩ Sub-shift: {}s (legendas serão deslocadas antes de embutir)", shift);
    }
    if args.sub_sync {
        println!("🔄 Sub-sync: offset automático via áudio");
    }
    println!();

    let plat = platform::detect()?;
    let icon = if cfg!(target_os = "macos") { "🍎" } else { "🐧" };
    println!("{} Sistema: {}", icon, plat.name);

    let tmdb_key = args.tmdb_key.clone().or_else(|| std::env::var("TMDB_API_KEY").ok());
    let tmdb_client = tmdb_key.filter(|k| !k.is_empty()).map(tmdb::TmdbClient::new);

    let organize_only = args.organize_only;
    let organize = args.organize || organize_only;

    let mut completed: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let total = files.len();

    for (i, input) in files.iter().enumerate() {
        let current = i + 1;
        println!();
        println!("═══════════════════════════════════════════════════════════");
        println!("📹 [{}/{}] Processando: {}", current, total, input.display());
        println!("═══════════════════════════════════════════════════════════");

        let src_name = input.file_name().unwrap().to_string_lossy().to_string();
        let legenda = input.with_extension("srt");
        std::fs::create_dir_all("output").ok();

        let clean = clean_name(input);
        let output = PathBuf::from(format!("output/{}.mkv", clean));

        let mut tmdb_meta: Option<TmdbMeta> = None;
        if organize {
            if let Some(client) = &tmdb_client {
                match client.resolve_for_file(&src_name) {
                    Ok(meta) => tmdb_meta = meta,
                    Err(e) => eprintln!("⚠️  TMDB falhou: {e}"),
                }
            }
        }

        if organize_only {
            match organize::organize_output(input, &src_name, tmdb_meta.as_ref(), true) {
                Ok(_) => completed.push(input.display().to_string()),
                Err(e) => {
                    eprintln!("❌ Erro ao organizar: {e}");
                    failed.push(input.display().to_string());
                }
            }
            continue;
        }

        // --- título ---
        let mut title_args: Vec<String> = Vec::new();
        if args.title_eng {
            match ffmpeg::probe(input) {
                Ok(info) => {
                    if let Some(t) = &info.current_title {
                        if let Some((_, eng)) = t.split_once(" / ") {
                            println!("🏷️  Título: '{}'", eng);
                            title_args = vec!["-metadata".into(), format!("title={}", eng)];
                        } else {
                            println!("🏷️  Título (fallback filename): '{}'", clean);
                            title_args = vec!["-metadata".into(), format!("title={}", clean)];
                        }
                    } else {
                        title_args = vec!["-metadata".into(), format!("title={}", clean)];
                    }
                }
                Err(_) => {
                    title_args = vec!["-metadata".into(), format!("title={}", clean)];
                }
            }
        } else if args.title_from_file {
            println!("🏷️  Título: '{}'", clean);
            title_args = vec!["-metadata".into(), format!("title={}", clean)];
        }

        if output.exists() {
            print!("⚠️  Arquivo de saída já existe: {} — sobrescrever? (s/n): ", output.display());
            std::io::stdout().flush().ok();
            let mut reply = String::new();
            std::io::stdin().read_line(&mut reply).ok();
            if !reply.trim().eq_ignore_ascii_case("s") {
                println!("⏭️  Pulando arquivo...");
                continue;
            }
        }

        let meta_args = tmdb_meta.as_ref().map(|m| m.to_metadata_args()).unwrap_or_default();

        // ─────────────────────────────────────────────
        // MODO SUB-ONLY: remux sem re-encode
        // ─────────────────────────────────────────────
        if args.sub_only {
            if !legenda.exists() {
                println!("⚠️  Legenda não encontrada: {} — pulando", legenda.display());
                failed.push(input.display().to_string());
                continue;
            }

            match prepare_subtitle(&legenda, input, &args) {
                Ok(final_srt) => {
                    let mut map_args = vec!["-map".into(), "0:v".into()];
                    map_args.extend(build_audio_map(&args.audio_lang, &args.audio_index));
                    if let Some(sl) = &args.sub_lang {
                        map_args.push("-map".into());
                        map_args.push(format!("0:s:m:language:{}", sl));
                        println!("💬 Legenda embarcada lang={} preservada", sl);
                    }
                    map_args.push("-map".into());
                    map_args.push("1:s:0".into());

                    let mut all_meta = title_args.clone();
                    all_meta.extend(meta_args.clone());

                    println!("🚀 Embutindo legenda (sem re-encode)...");
                    println!("--------------------------------------------------");
                    let ok = ffmpeg::run_remux(input, &[final_srt.as_path()], &map_args, &all_meta, &output)?;
                    let _ = std::fs::remove_file(&final_srt);

                    if ok {
                        println!("✅ [{}/{}] Legenda embutida com sucesso!", current, total);
                        if organize {
                            let _ = organize::organize_output(&output, &src_name, tmdb_meta.as_ref(), false);
                        }
                        completed.push(input.display().to_string());
                    } else {
                        println!("❌ [{}/{}] Erro ao embutir legenda!", current, total);
                        failed.push(input.display().to_string());
                    }
                }
                Err(e) => {
                    eprintln!("❌ Erro processando legenda: {e}");
                    failed.push(input.display().to_string());
                }
            }
            continue;
        }

        // ─────────────────────────────────────────────
        // MODO NORMAL: re-encode HEVC
        // ─────────────────────────────────────────────
        println!("🔍 Analisando mídia...");
        let info = match ffmpeg::probe(input) {
            Ok(i) => i,
            Err(e) => {
                println!("❌ Erro ao analisar o arquivo: {input:?} ({e})");
                failed.push(input.display().to_string());
                continue;
            }
        };

        let mut hdr_opts: Vec<String> = Vec::new();
        if info.color_transfer == "smpte2084" {
            println!("✨ HDR10 Detectado.");
            hdr_opts = vec![
                "-color_primaries".into(), "bt2020".into(),
                "-color_trc".into(), "smpte2084".into(),
                "-colorspace".into(), "bt2020nc".into(),
            ];
            if let (Some(cll), Some(fall)) = (&info.max_cll, &info.max_fall) {
                if cll != "0" {
                    hdr_opts.push("-metadata:s:v:0".into());
                    hdr_opts.push(format!("max-cll={}", cll));
                    hdr_opts.push("-metadata:s:v:0".into());
                    hdr_opts.push(format!("max-fall={}", fall));
                }
            }
        }

        let mut map_args = vec!["-map".to_string(), "0:v:0".to_string()];
        map_args.extend(build_audio_map(&args.audio_lang, &args.audio_index));

        if let Some(sl) = &args.sub_lang {
            map_args.push("-map".into());
            map_args.push(format!("0:s:m:language:{}", sl));
        }

        let mut extra_inputs: Vec<PathBuf> = Vec::new();
        let mut srt_tmp: Option<PathBuf> = None;
        if legenda.exists() {
            println!("🎯 Legenda encontrada: {}", legenda.display());
            let final_srt = prepare_subtitle(&legenda, input, &args)?;
            map_args.push("-map".into());
            map_args.push("1:s:0".into());
            extra_inputs.push(final_srt.clone());
            srt_tmp = Some(final_srt);
        }

        let mut all_meta = title_args.clone();
        all_meta.extend(meta_args.clone());

        println!("🚀 Iniciando encode: {} @ {}", tune.name, tune.bitrate);
        let est_min = (info.total_frames as f64 / 50.0 / 60.0) as i64;
        println!(
            "⏱️  Duração: {} min | Espera est.: ~{} min",
            (info.duration_secs / 60.0) as i64,
            est_min
        );
        println!("--------------------------------------------------");

        let extra_refs: Vec<&Path> = extra_inputs.iter().map(|p| p.as_path()).collect();
        let enc_args = ffmpeg::EncodeArgs {
            input,
            extra_inputs: extra_refs,
            map_args,
            vf_opts: plat.vf_opts,
            encoder: plat.encoder,
            vaapi_device: plat.vaapi_device,
            rc_mode: plat.rc_mode,
            bitrate: tune.bitrate,
            maxrate: tune.maxrate,
            bufsize: tune.bufsize,
            hdr_opts,
            metadata_args: all_meta,
            output: &output,
            total_frames: info.total_frames,
        };

        let ok = ffmpeg::run_encode(&enc_args)?;
        if let Some(tmp) = srt_tmp {
            let _ = std::fs::remove_file(tmp);
        }

        println!("--------------------------------------------------");
        if ok {
            println!("✅ [{}/{}] Concluído com sucesso!", current, total);
            if organize {
                let _ = organize::organize_output(&output, &src_name, tmdb_meta.as_ref(), false);
            }
            completed.push(input.display().to_string());
        } else {
            println!("❌ [{}/{}] Erro durante o encode!", current, total);
            failed.push(input.display().to_string());
        }
    }

    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("📊 RESUMO DA FILA");
    println!("═══════════════════════════════════════════════════════════");
    println!("✅ Arquivos concluídos: {}/{}", completed.len(), total);

    if !completed.is_empty() {
        println!();
        println!("Arquivos processados com sucesso:");
        for f in &completed {
            println!("  ✓ {}", f);
        }
    }
    if !failed.is_empty() {
        println!();
        println!("❌ Arquivos com erro ({}):", failed.len());
        for f in &failed {
            println!("  ✗ {}", f);
        }
    }
    println!();
    println!("═══════════════════════════════════════════════════════════");

    if !failed.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

/// Pipeline: normaliza encoding -> sub-sync -> sub-shift, retorna path do .srt final temporário.
fn prepare_subtitle(legenda: &Path, video: &Path, args: &cli::Args) -> Result<PathBuf> {
    let tmp_dir = std::env::temp_dir();
    let mut current = legenda.to_path_buf();

    let enc = subtitle::detect_encoding(legenda).context("detectando encoding da legenda")?;
    if enc != encoding_rs::UTF_8 {
        let norm = tmp_dir.join(format!("norm_{}.srt", std::process::id()));
        subtitle::normalize_to_utf8(&current, &norm, enc)?;
        println!("🔤 Encoding convertido: {} → utf-8", enc.name());
        current = norm;
    }

    if args.sub_sync {
        let info = ffmpeg::probe(video).context("probing vídeo para sub-sync")?;
        let synced = tmp_dir.join(format!("synced_{}.srt", std::process::id()));
        subtitle::auto_sync(&current, &synced, info.start_time_ms)?;
        current = synced;
    }

    if let Some(shift) = args.sub_shift {
        let shifted = tmp_dir.join(format!("shifted_{}.srt", std::process::id()));
        subtitle::shift_srt(&current, &shifted, shift)?;
        println!("⏩ Legenda deslocada {}s adicional", shift);
        current = shifted;
    }

    Ok(current)
}
