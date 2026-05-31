# LysineOS

An AI-native Linux operating system for developers, built from scratch.

## Overview

LysineOS is a custom Linux distribution built entirely from source (based on Linux From Scratch 13.0), featuring:

- **Ribosome Build System** - A custom build engine and package manager with biology-inspired naming
- **JARVIS-style HUD Desktop** - Holographic UI with arc reactor animations, waveform visualizations, and translucent panels
- **Deep AI Integration** - Voice wake word, conversation, visual understanding (local-first + optional cloud)
- **Developer-centric** - Built-in container support, multi-language toolchains, tiling window management

## Architecture

The full architecture document is available at [docs/architecture.md](docs/architecture.md).

## Ribosome Build System

LysineOS uses a custom build system named **Ribosome**, inspired by the biological process of protein synthesis:

| Component | Name | Description |
|-----------|------|-------------|
| Build Engine | `ribosome` | Core build daemon (Rust) |
| Build Recipe | `mRNA` | Declarative YAML build descriptions |
| Package Manager | `lysin` | Install/remove/query packages |
| Binary Package | `.protein` | Compiled package format |
| Repository | `nucleus` | Software repository server |
| Build Sandbox | `membrane` | Namespace/cgroup isolation |
| Dependency Graph | `genome` | DAG-based dependency resolution |
| System Snapshot | `mitosis` | Btrfs snapshot/rollback |

## Tech Stack

- **Kernel**: Linux 6.x (custom-tuned)
- **Init**: systemd
- **Compositor**: Smithay (Rust) - Wayland
- **Desktop Shell**: Custom wgpu-based HUD renderer
- **AI**: Ollama + whisper.cpp + Piper TTS + LLaVA
- **Language**: Primarily Rust

## Project Status

Currently in planning phase. See [docs/architecture.md](docs/architecture.md) for the full roadmap.

## License

MIT
