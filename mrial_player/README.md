# Overview
Player for the Mrial Server using Slint + FFmpeg

# Run

## MacOS

1. Download the latest `Mrial.dmg` file from the releases tab.
2. You will need to run `xattr -c Mrial.app` once to open the app (this is because the app is no longer signed and notarized via Apple).

## Linux (Debain)

1. Follow mrial_server [README.md](../mrial_server/README.md), it will install both the app and server together.

# Compile + Run

## MacOS
1. `brew install rust yasm ffmpeg`
2. `RUST_LOG=mrial_player=debug cargo run --release --features build_ffmpeg` (for debugging)
