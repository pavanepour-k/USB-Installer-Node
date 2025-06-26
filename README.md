# USB-Installer-Node

**A bootable USB node for remote-controlled OS installation on headless PCs**  
_Remotely deploy Linux, Windows, or BSD with no monitor, keyboard, or mouse required._

---

## Table of Contents

1. [Overview](#overview)  
2. [Features](#features)  
3. [How It Works](#how-it-works)  
4. [Device Requirements](#device-requirements)  
5. [Roadmap](#roadmap)  
6. [License](#license)

---

## Overview

**USB-Installer-Node** allows you to boot and remotely install an OS on an unconfigured desktop (no OS, no peripherals) using a USB device as a self-contained control node.

It includes:
- A custom Live Linux/BSD environment
- Preloaded OS installation ISOs (Windows, Linux, BSD)
- Remote control services (SSH, VNC, noVNC)

---

## Features

- **Zero-peripheral installation**  
- **Remote VNC (GUI) + SSH (terminal)** access  
- **Preconfigured networking (DHCP or static)**  
- **Auto-mounted ISO installer**  
- **Multi-OS deployment**  
- **Works on offline machines (ISOs bundled)**  
- **Browser-based control interface**

---

## How It Works

1. Plug the USB into target PC (desktop-2)
2. Boot from USB (BIOS/UEFI boot)
3. USB auto-configures network, starts remote services
4. Control the PC remotely from desktop-1 (via browser or VNC/SSH)
5. Select and run the desired OS installer

---

## Device Requirements

### Desktop-1 (Control PC)
- OS: Windows, Linux, macOS
- Browser or VNC client installed

### Desktop-2 (Target PC)
- Can boot from USB
- Keyboard/monitor not required
- Network port available (Ethernet preferred)

### USB Device
- At least 16GB (32GB+ recommended)
- Writable (for logs, updates)


---


 Use Cases

    Remote OS recovery or reinstallation

    Unattended provisioning for headless servers
    
    **Have an old laptop with the screen and keyboard long gone, just the chassis left — and setting it up feels like too much work? This tool might help. Maybe. No promises it’ll work perfectly.**



## Roadmap

Web-based OS selector UI

PXE mode (network-only fallback)

Secure remote token-based access

Offline GUI installer (w/ Wi-Fi config)

Encrypted USB build option



## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
