# Changelog
All notable changes to this project are documented in this file.

## 0.3.0 - TBD

- Web Support - (WIP)

## 0.2.1 - TBD

- Cursor on client should match the host's cursor type - (WIP)

## 0.2.0 - TBD

- Major Proto Upgrade (Supports Retransmissions) and highly optimized
- Opus Audio compression
- Version between server and client must be the same 
- YUV Selection (420 or 444)
- Outlined backend for eventual web support 
- Reconnection UI

## 0.1.18 - 2024-02-17

- Logout detection with 1 second frequency
- No longer dependent on libxdo
- Mrial Player is included in .deb package (since the player application is needed for creating users)
- Simple CLI part of the mrial_server binary (use mrial_server --help to learn more)
- The UI and the CLI both store authenticated users at /var/lib/mrial_server directory on linux

## 0.1.17 - 2024-02-10

- Systemd service works again! (well... at least partially)
- The server is now meant to be run from root and supports lightdm display manager (the goal is to eventually support all major display managers)
- The server is also able to connect to Pipewire from root (audio support)
- Automatic login detection is enabled with 1 second frequency
- Logout detection still doesn't exist, right now the server crashes when you log out and restarts automatically (if using systemd) on the login page, super hacky right 