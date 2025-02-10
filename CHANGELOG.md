# Changelog
All notable changes to this project are documented in this file.

## 0.1.17 - 2024-02-10

- Systemd service works again! (well... at least partially)
- The server is now meant to be run from root and supports lightdm display manager (the goal is to eventually support all major display managers)
- The server is also able to connect to Pipewire from root (audio support)
- Automatic login detection is enabled with 1 second frequency
- Logout detection still doesn't exist, right now the server crashes when you log out and restarts automatically (if using systemd) on the login page, super hacky right 