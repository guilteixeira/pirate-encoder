use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "pirate-encoder",
    about = "Encoder/organizador de mídia com aceleração de hardware (VideoToolbox/VAAPI)",
    override_usage = "pirate-encoder [ARQUIVOS...] [OPÇÕES]"
)]
pub struct Args {
    /// Arquivos ou wildcards (ex: '*.mkv')
    #[arg(required = true)]
    pub files: Vec<String>,

    /// Preset de bitrate: anime, action, cult
    #[arg(long)]
    pub tune: Option<String>,

    /// Apenas embute legenda, sem re-encode
    #[arg(long)]
    pub sub_only: bool,

    /// Detecta offset automaticamente e corrige (via start_time do container)
    #[arg(long)]
    pub sub_sync: bool,

    /// Desloca legenda em N segundos (ex: -3.5, +2)
    #[arg(long, allow_hyphen_values = true)]
    pub sub_shift: Option<f64>,

    /// Idiomas de áudio a manter, em ordem, separados por vírgula (ex: por,eng)
    #[arg(long)]
    pub audio_lang: Option<String>,

    /// Índice (0-based) da faixa de áudio a manter
    #[arg(long)]
    pub audio_index: Option<usize>,

    /// Idioma da legenda embarcada no container a preservar
    #[arg(long)]
    pub sub_lang: Option<String>,

    /// Organiza output em pastas via TMDB após o encode
    #[arg(long)]
    pub organize: bool,

    /// Apenas organiza arquivos já existentes em output/, sem re-encode
    #[arg(long)]
    pub organize_only: bool,

    /// Chave da API TMDB (ou defina TMDB_API_KEY no ambiente)
    #[arg(long)]
    pub tmdb_key: Option<String>,

    /// Sobrescreve título pelo nome do arquivo
    #[arg(long)]
    pub title_from_file: bool,

    /// Extrai título em inglês (parte após " / " na tag existente)
    #[arg(long)]
    pub title_eng: bool,
}

pub struct TunePreset {
    pub name: &'static str,
    pub bitrate: &'static str,
    pub maxrate: &'static str,
    pub bufsize: &'static str,
}

pub fn resolve_tune(tune: &Option<String>) -> TunePreset {
    match tune.as_deref() {
        Some("anime") => TunePreset { name: "anime", bitrate: "5M", maxrate: "8M", bufsize: "10M" },
        Some("action") => TunePreset { name: "action", bitrate: "10M", maxrate: "18M", bufsize: "20M" },
        Some("cult") => TunePreset { name: "cult", bitrate: "12M", maxrate: "20M", bufsize: "25M" },
        _ => TunePreset { name: "normal", bitrate: "6.5M", maxrate: "10M", bufsize: "13M" },
    }
}
