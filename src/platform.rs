/// Configuração de encoder dependente do SO, equivalente ao case "${OS}" do bash original.
pub struct PlatformConfig {
    pub name: &'static str,
    pub encoder: &'static str,
    pub vf_opts: &'static str,
    pub vaapi_device: Option<&'static str>,
    pub rc_mode: Option<&'static str>,
}

pub fn detect() -> anyhow::Result<PlatformConfig> {
    if cfg!(target_os = "macos") {
        Ok(PlatformConfig {
            name: "macOS (VideoToolbox)",
            encoder: "hevc_videotoolbox",
            vf_opts: "format=p010",
            vaapi_device: None,
            rc_mode: None,
        })
    } else if cfg!(target_os = "linux") {
        Ok(PlatformConfig {
            name: "Linux (VAAPI)",
            encoder: "hevc_vaapi",
            vf_opts: "format=p010,hwupload",
            vaapi_device: Some("/dev/dri/renderD128"),
            rc_mode: Some("VBR"),
        })
    } else {
        anyhow::bail!("Sistema não suportado")
    }
}
