# pirate-encoder

Script para encoding amador para agilizar backups e dumps de dvds antigos

## Build

```bash
cargo build --release
./target/release/pirate-encoder --help
```

O binário fica em `target/release/pirate-encoder`. Copie para algum lugar no PATH se quiser
(ex: `cp target/release/pirate-encoder /usr/local/bin/`).

## Uso

```bash
./target/release/pirate-encoder '*.mkv' --tune anime
./target/release/pirate-encoder '*.mkv' --sub-only --sub-sync
./target/release/pirate-encoder '*.mkv' --audio-lang por,eng --sub-lang por
./target/release/pirate-encoder '*.mkv' --organize --tmdb-key SUA_KEY
```

`cargo run --release -- '*.mkv' --tune anime` também funciona direto, sem precisar
buildar antes (cargo builda e roda em um passo só).

## Dependências

- Runtime: `ffmpeg`, `ffprobe`, `curl` na ultima versão disponível.
- Build: toolchain Rust estável (`rustup` ou `brew install rust` no macOS).

## Testes

```bash
cargo test
```
