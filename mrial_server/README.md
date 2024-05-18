# Install

# Windows

1. choco install pkgconfiglite (to install pkg-config)
2. Follow the instructions found on this website to compile x264 (https://www.roxlu.com/2016/057/compiling-x264-on-windows-with-msvc)

# Linux

1. Install libxrandr-dev, libxcb-randr0-dev
2. Make sure user is added to display manager's group (such as lightdm's)

# Run Server
1. Install libxdo-dev
2. export XAUTHORITY=/var/lib/lightdm/.Xauthority 
3. export DISPLAY=:0
4. sudo xdotool type "password"
5. sudo xdotool key Return
6. ./mrial_server