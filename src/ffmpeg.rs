use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

pub struct MediaInfo {
    pub duration_secs: f64,
    pub total_frames: i64,
    pub start_time_ms: i64,
    pub color_transfer: String,
    pub max_cll: Option<String>,
    pub max_fall: Option<String>,
    pub current_title: Option<String>,
}

fn ffprobe_json(args: &[&str]) -> Result<Value> {
    let out = Command::new("ffprobe")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .context("executando ffprobe (está no PATH?)")?;
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    Ok(v)
}

/// Faz uma única chamada de ffprobe em modo JSON coletando tudo que o script bash
/// buscava em várias invocações separadas (duration, start_time, r_frame_rate,
/// color_transfer, side_data HDR, tag de título) — mais rápido e mais robusto
/// que fazer grep/sed em texto "default=noprint_wrappers".
pub fn probe(input: &Path) -> Result<MediaInfo> {
    let input_str = input.to_string_lossy().to_string();

    let fmt = ffprobe_json(&[
        "-v", "error",
        "-show_entries", "format=duration,start_time:format_tags=title",
        "-print_format", "json",
        &input_str,
    ])?;

    let format = &fmt["format"];
    let duration_secs: f64 = format["duration"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    if duration_secs <= 0.0 {
        bail!("não foi possível determinar a duração do arquivo");
    }
    let start_time_ms = (format["start_time"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
        * 1000.0)
        .round() as i64;
    let start_time_ms = start_time_ms.max(0);
    let current_title = format["tags"]["title"].as_str().map(|s| s.to_string());

    let stream = ffprobe_json(&[
        "-v", "error",
        "-select_streams", "v:0",
        "-show_entries", "stream=r_frame_rate,color_transfer",
        "-print_format", "json",
        &input_str,
    ])?;
    let stream0 = &stream["streams"][0];
    let fps_raw = stream0["r_frame_rate"].as_str().unwrap_or("24/1");
    let fps = parse_fraction(fps_raw).unwrap_or(24.0);
    let color_transfer = stream0["color_transfer"].as_str().unwrap_or("").to_string();

    let total_frames = (fps * duration_secs).round() as i64;

    let mut max_cll = None;
    let mut max_fall = None;
    if color_transfer == "smpte2084" {
        let frames = ffprobe_json(&[
            "-v", "error",
            "-select_streams", "v:0",
            "-show_frames",
            "-read_intervals", "%+#1",
            "-show_entries", "frame=side_data_list",
            "-print_format", "json",
            &input_str,
        ])?;
        if let Some(side_list) = frames["frames"][0]["side_data_list"].as_array() {
            for sd in side_list {
                if let Some(v) = sd.get("max_content").or_else(|| sd.get("max_content_light_level")) {
                    max_cll = v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_string));
                }
                if let Some(v) = sd.get("max_average").or_else(|| sd.get("max_average_light_level")) {
                    max_fall = v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_string));
                }
            }
        }
    }

    Ok(MediaInfo {
        duration_secs,
        total_frames,
        start_time_ms,
        color_transfer,
        max_cll,
        max_fall,
        current_title,
    })
}

fn parse_fraction(s: &str) -> Option<f64> {
    if let Some((n, d)) = s.split_once('/') {
        let n: f64 = n.parse().ok()?;
        let d: f64 = d.parse().ok()?;
        if d > 0.0 {
            return Some(n / d);
        }
        return None;
    }
    s.parse().ok()
}

pub struct EncodeArgs<'a> {
    pub input: &'a Path,
    pub extra_inputs: Vec<&'a Path>, // ex: legenda externa
    pub map_args: Vec<String>,
    pub vf_opts: &'a str,
    pub encoder: &'a str,
    pub vaapi_device: Option<&'a str>,
    pub rc_mode: Option<&'a str>,
    pub bitrate: &'a str,
    pub maxrate: &'a str,
    pub bufsize: &'a str,
    pub hdr_opts: Vec<String>,
    pub metadata_args: Vec<String>,
    pub output: &'a Path,
    pub total_frames: i64,
}

/// Executa o encode usando `-progress pipe:1`, que emite pares key=value
/// legíveis por máquina — em vez de re-fazer grep no log inteiro a cada
/// tick (o que no script bash ficava mais lento à medida que o arquivo de
/// log crescia). Aqui o progresso é O(1) por atualização.
pub fn run_encode(args: &EncodeArgs) -> Result<bool> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-loglevel").arg("error");

    if let Some(dev) = args.vaapi_device {
        cmd.arg("-vaapi_device").arg(dev);
    }

    cmd.arg("-i").arg(args.input);
    for extra in &args.extra_inputs {
        cmd.arg("-i").arg(extra);
    }

    for m in &args.map_args {
        cmd.arg(m);
    }

    cmd.arg("-vf").arg(args.vf_opts);
    cmd.arg("-c:v").arg(args.encoder);
    cmd.arg("-profile:v").arg("main10");
    cmd.arg("-b:v").arg(args.bitrate);
    cmd.arg("-maxrate:v").arg(args.maxrate);
    cmd.arg("-bufsize:v").arg(args.bufsize);
    if let Some(rc) = args.rc_mode {
        cmd.arg("-rc_mode").arg(rc);
    }
    for h in &args.hdr_opts {
        cmd.arg(h);
    }
    cmd.arg("-c:a").arg("copy");
    cmd.arg("-c:s").arg("copy");
    for m in &args.metadata_args {
        cmd.arg(m);
    }
    cmd.arg("-progress").arg("pipe:1");
    cmd.arg("-nostats");
    cmd.arg("-y").arg(args.output);

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("executando ffmpeg (está no PATH?)")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Captura stderr em thread separada para exibir em caso de erro, sem bloquear o progresso.
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr);
        use std::io::Read;
        let _ = reader.read_to_string(&mut buf);
        buf
    });

    let pb = ProgressBar::new(args.total_frames.max(1) as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "⏳ {elapsed_precise} | frame={pos}/{len} fps={msg} [{percent}%] ETA {eta_precise}",
        )
        .unwrap(),
    );

    let start = Instant::now();
    let reader = BufReader::new(stdout);
    let mut frame: i64 = 0;
    let mut fps: f64 = 0.0;

    for line in reader.lines() {
        let line = line?;
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "frame" => frame = v.trim().parse().unwrap_or(frame),
                "fps" => fps = v.trim().parse().unwrap_or(fps),
                "progress" => {
                    pb.set_position(frame.min(args.total_frames.max(1)) as u64);
                    pb.set_message(format!("{:.0}", fps));
                    if v.trim() == "end" {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    pb.finish_and_clear();
    let elapsed = start.elapsed();

    let status = child.wait().context("aguardando ffmpeg")?;
    let stderr_out = stderr_handle.join().unwrap_or_default();

    println!(
        "⏱️  Tempo de encode: {:02}:{:02}",
        elapsed.as_secs() / 60,
        elapsed.as_secs() % 60
    );

    if !status.success() {
        println!("   Log ffmpeg:");
        for l in stderr_out.lines().filter(|l| !l.trim().is_empty()) {
            println!("   {}", l);
        }
    }

    Ok(status.success())
}

/// Remux simples (sub-only / embed de metadados) sem re-encode de vídeo/áudio.
pub fn run_remux(
    input: &Path,
    extra_inputs: &[&Path],
    map_args: &[String],
    metadata_args: &[String],
    output: &Path,
) -> Result<bool> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-loglevel").arg("error");
    cmd.arg("-i").arg(input);
    for e in extra_inputs {
        cmd.arg("-i").arg(e);
    }
    for m in map_args {
        cmd.arg(m);
    }
    cmd.arg("-c").arg("copy");
    cmd.arg("-c:s").arg("copy");
    for m in metadata_args {
        cmd.arg(m);
    }
    cmd.arg("-y").arg(output);
    cmd.stderr(Stdio::piped());
    let out = cmd.output().context("executando ffmpeg")?;
    if !out.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(out.status.success())
}
