# Overview

Mrial Server

# Setup clean Ubuntu VM with Mrial (Production)

1. `sudo apt install ubuntu-desktop`
2. Install LightDM display manager via `sudo apt-get install lightdm`
3. Ensure LightDM (or your display-manager) is running before proceeding (`sudo systemctl start display-manager`)

# Build (Development)

## Windows Requirements

1. choco install pkgconfiglite (to install pkg-config)
2. Follow the instructions found on this website to compile x264 (https://www.roxlu.com/2016/057/compiling-x264-on-windows-with-msvc)

## Linux Requirements

1. Requires GCC v14 or higher or Clang v18 or higher
2. Install libxrandr-dev, libxcb-randr0-dev

# Run (Development)

1. cargo run