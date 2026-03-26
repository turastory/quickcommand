# quickcommand

Local-first natural language shell assistant for macOS.

`quickcommand` generates shell commands from plain language using a local Ollama model.
The installed CLI binary is `qc`.

## Install

### Homebrew

```bash
brew tap turastory/tap
brew install qc
qc init
```

### Build from source

```bash
cargo build --release
./target/release/qc --help
```

## Usage

```bash
qc "show the current working directory"
qc --execute "show the current working directory"
qc init
qc config show
```

After `qc init`, zsh integration is installed into `~/.zshrc`. A normal `qc "..."` call will then populate the next zsh prompt with the generated command so you can edit it and press Enter to run it.

## Release asset

Homebrew installs a prebuilt GitHub Release asset named:

```text
qc-aarch64-apple-darwin.tar.gz
```

The archive contains a single binary:

```text
qc
```
