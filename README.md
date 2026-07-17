# pirate-encoder (Rust)

Reescrita nativa do `pirate-encoder.sh`, mesma funcionalidade, sem overhead de shell.

## Build

```bash
cargo build --release
./target/release/pirate-encoder --help
```

O binário fica em `target/release/pirate-encoder`. Copie para algum lugar no PATH se quiser
(ex: `cp target/release/pirate-encoder /usr/local/bin/`).

## Uso

Mesma sintaxe do script bash:

```bash
./target/release/pirate-encoder '*.mkv' --tune anime
./target/release/pirate-encoder '*.mkv' --sub-only --sub-sync
./target/release/pirate-encoder '*.mkv' --audio-lang por,eng --sub-lang por
./target/release/pirate-encoder '*.mkv' --organize --tmdb-key SUA_KEY
```

`cargo run --release -- '*.mkv' --tune anime` também funciona direto, sem precisar
buildar antes (cargo builda e roda em um passo só).

## O que mudou em relação ao bash

- **Progresso**: usa `ffmpeg -progress pipe:1` (saída estruturada key=value) em vez de
  re-fazer `grep`/`tail` no log inteiro a cada 0.5s. O script bash ficava mais lento à
  medida que o log crescia (rescan O(n) do arquivo a cada tick); aqui é O(1) por update.
- **Probing**: uma chamada de `ffprobe` em JSON por arquivo (duration, fps, HDR,
  título) em vez de 5-6 chamadas separadas com `-of default` + grep/sed.
- **TMDB**: resposta JSON parseada com `serde_json` (não quebra mais com aspas/vírgulas
  dentro de overview/título). Ainda usa `curl` como subprocess (mesma dependência
  externa que o script original já tinha) para evitar puxar uma stack HTTP pesada.
- **Legendas**: detecção de encoding via `chardetng` e conversão via `encoding_rs`,
  nativos em Rust — sem spawnar `file`/`iconv` por arquivo. Shift e sub-sync são a
  mesma lógica do bash, portada 1:1 (testada com `cargo test`).
- **Import importante**: o encode em si (VideoToolbox/VAAPI) roda exatamente igual —
  isso é trabalho de hardware fixo, então a velocidade de encode não muda. O que
  melhora é tudo em volta (orquestração, parsing, TMDB, legendas), que era a parte
  crescendo devagar conforme você adicionava features ao monólito bash.

## Dependências

- Runtime: `ffmpeg`, `ffprobe`, `curl` no PATH (mesmas do script original).
- Build: toolchain Rust estável (`rustup` ou `brew install rust` no macOS).

## Testes

```bash
cargo test
```

Cobre a lógica de shift/sync de legendas (a parte mais delicada de portar), validada
contra os mesmos casos do bash original.
