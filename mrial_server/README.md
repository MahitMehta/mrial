# Overview

Mrial Server

# Setup clean Ubuntu VM with Mrial (Production)

1. `sudo apt install ubuntu-desktop`
2. Install LightDM display manager via `sudo apt-get install lightdm`
3. Ensure LightDM (or your display-manager) is running before proceeding (`sudo systemctl start display-manager`)
4. Now, wget the latest `.deb` package of Mrial from Github and install it!
5. Once the server is running, you will need to add at least one authenticated user
6. Run `/usr/bin/mrial_server user add [username] [password]`
7. Now, install the the Mrial Player and connect! (by default the server is hosted on port `8554`)
8. To get audio to work, follow the steps on how to setup Pipewire!

# Setup Audio via Pipewire on Ubuntu (Production)

1. sudo add-apt-repository ppa:pipewire-debian/pipewire-upstream
2. sudo apt update
3. sudo apt install pipewire
4. sudo systemctl restart mrial-server

# Build (Development)

## Linux Requirements

1. Requires GCC v14 or higher or Clang v18 or higher
2. Install libxrandr-dev, libxcb-randr0-dev

## Windows Requirements

1. choco install pkgconfiglite (to install pkg-config)
2. Follow the instructions found on this website to compile x264 (https://www.roxlu.com/2016/057/compiling-x264-on-windows-with-msvc)


# Run (Development)

1. cargo run