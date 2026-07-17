use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;
use std::process::Command;

#[derive(Default, Clone)]
pub struct TmdbMeta {
    pub title: Option<String>,
    pub year: Option<String>,
    pub id: Option<i64>,
    pub overview: Option<String>,
    pub genres: Option<String>,
}

impl TmdbMeta {
    /// Monta os pares -metadata equivalentes a TMDB_META_ARGS no bash.
    pub fn to_metadata_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(t) = &self.title {
            args.push("-metadata".into());
            args.push(format!("title={}", t));
        }
        if let Some(y) = &self.year {
            args.push("-metadata".into());
            args.push(format!("date={}", y));
        }
        if let Some(o) = &self.overview {
            args.push("-metadata".into());
            args.push(format!("comment={}", o));
        }
        if let Some(g) = &self.genres {
            args.push("-metadata".into());
            args.push(format!("genre={}", g));
        }
        args
    }
}

pub struct TmdbClient {
    key: String,
    bearer: bool,
}

impl TmdbClient {
    pub fn new(key: String) -> Self {
        let bearer = key.len() > 100;
        TmdbClient { key, bearer }
    }

    /// GET autenticado via `curl`, parseando a resposta como JSON com serde_json
    /// (em vez do grep/sed campo-a-campo do script original — mais robusto e
    /// imune a valores com caracteres especiais).
    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<Value> {
        let mut all_query: Vec<(String, String)> =
            query.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        if !self.bearer {
            all_query.push(("api_key".to_string(), self.key.clone()));
        }

        let mut cmd = Command::new("curl");
        cmd.arg("-sf").arg("-G");
        cmd.arg("-H").arg("accept: application/json");
        if self.bearer {
            cmd.arg("-H").arg(format!("Authorization: Bearer {}", self.key));
        }
        for (k, v) in &all_query {
            cmd.arg("--data-urlencode").arg(format!("{}={}", k, v));
        }
        cmd.arg(url);

        let out = cmd.output().context("executando curl (está instalado?)")?;
        if !out.status.success() || out.stdout.is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_slice(&out.stdout).unwrap_or(Value::Null))
    }

    /// Busca por título (filme ou série), retorna (título, ano, id).
    pub fn search(&self, is_tv: bool, query: &str, year: Option<&str>) -> Result<Option<(String, String, i64)>> {
        let path = if is_tv { "search/tv" } else { "search/movie" };
        let url = format!("https://api.themoviedb.org/3/{}", path);
        let mut params = vec![("query", query)];
        if let Some(y) = year {
            if !is_tv {
                params.push(("primary_release_year", y));
            }
        }
        let v = self.get(&url, &params)?;
        let results = v["results"].as_array().cloned().unwrap_or_default();
        let first = match results.first() {
            Some(r) => r,
            None => return Ok(None),
        };

        let title_key = if is_tv { "name" } else { "title" };
        let date_key = if is_tv { "first_air_date" } else { "release_date" };

        let title = first[title_key].as_str().unwrap_or("").to_string();
        let date = first[date_key].as_str().unwrap_or("");
        let yr = date.get(0..4).unwrap_or("").to_string();
        let id = first["id"].as_i64().unwrap_or(0);

        if title.is_empty() || yr.is_empty() || id == 0 {
            return Ok(None);
        }
        Ok(Some((title, yr, id)))
    }

    fn detail(&self, url: &str) -> Result<Value> {
        self.get(url, &[("language", "pt-BR")])
    }

    /// Equivalente a resolve_tmdb_for_file(): parseia nome do arquivo, busca,
    /// e traz overview + gêneros.
    pub fn resolve_for_file(&self, src_name: &str) -> Result<Option<TmdbMeta>> {
        let se_re = Regex::new(r"[Ss](\d{2})[Ee](\d{2})").unwrap();
        let year_re = Regex::new(r"(19\d{2}|20\d{2})").unwrap();

        let season_ep = se_re.captures(src_name);
        let year_match = year_re.captures(src_name).map(|c| c[1].to_string());

        let parsed = src_name.rsplit_once('.').map(|(a, _)| a).unwrap_or(src_name).replace('.', " ");

        let search_title = if let Some(caps) = &season_ep {
            let full = caps.get(0).unwrap().as_str();
            parsed.split(full).next().unwrap_or(&parsed).trim().to_string()
        } else if let Some(y) = &year_match {
            parsed.split(y.as_str()).next().unwrap_or(&parsed).trim().to_string()
        } else {
            let junk_re = Regex::new(
                r"(?i) (PROPER|REPACK|EXTENDED|2160p|1080p|720p|BluRay|WEB-DL|REMUX|HEVC|x265|x264|DTS|AAC|HDR|DV|Hybrid).*",
            )
            .unwrap();
            junk_re.replace(&parsed, "").trim().to_string()
        };

        let is_tv = season_ep.is_some();
        let search_result = self.search(is_tv, &search_title, year_match.as_deref())?;
        let (title, yr, id) = match search_result {
            Some(r) => r,
            None => {
                println!("   ⚠️  TMDB: sem resultado para \"{}\"", search_title);
                return Ok(None);
            }
        };
        println!("   🎬 TMDB: {} ({})", title, yr);

        let (detail_res, genre_res) = if let Some(caps) = &season_ep {
            let season_num = &caps[1];
            let episode_num = &caps[2];
            let d = self.detail(&format!(
                "https://api.themoviedb.org/3/tv/{}/season/{}/episode/{}",
                id, season_num, episode_num
            ))?;
            let g = self.detail(&format!("https://api.themoviedb.org/3/tv/{}", id))?;
            (d, g)
        } else {
            let d = self.detail(&format!("https://api.themoviedb.org/3/movie/{}", id))?;
            (d.clone(), d)
        };

        let overview = detail_res["overview"].as_str().filter(|s| !s.is_empty()).map(|s| s.replace('\n', " ").replace('\r', ""));
        let genres = genre_res["genres"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|g| g["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .filter(|s| !s.is_empty());

        if let Some(o) = &overview {
            println!("   📝 {}...", &o.chars().take(90).collect::<String>());
        }
        if let Some(g) = &genres {
            println!("   🏷️  Gêneros: {}", g);
        }

        Ok(Some(TmdbMeta {
            title: Some(title),
            year: Some(yr),
            id: Some(id),
            overview,
            genres,
        }))
    }
}
