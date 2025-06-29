# USB Installer Node Architecture

## Overview

The USB Installer Node is a Rust-based system designed to automate OS installation on target machines via USB boot. It provides network services, remote access capabilities, and automated installation workflows.

## Core Components

### 1. Network Layer (`src/network/`)
- **NetworkManager**: Orchestrates all network operations
- **DHCP Client**: Automatic IP configuration
- **Hostname Manager**: mDNS registration with auto-generated hostnames
- **Tunnel Manager**: VPN support (Tailscale, WireGuard, SSH)

### 2. Disk Management (`src/disk/`)
- **DiskManager**: High-level disk operations orchestration
- **Partitioner**: MBR/GPT partition table creation and management
- **Formatter**: Multi-filesystem support (ext4, xfs, btrfs, ntfs, vfat)

### 3. ISO Management (`src/iso/`)
- **IsoManager**: ISO discovery, mounting, and lifecycle management
- **Mounter**: Loop device mounting with state tracking
- **Installer**: OS-specific installer detection and execution

### 4. Remote Access (`src/remote/`)
- **RemoteManager**: Unified remote access control
- **VNC Server**: X11 desktop sharing with auto-restart
- **SSH Server**: Secure shell access with key management
- **Web VNC**: Browser-based remote access via noVNC

### 5. UI System (`src/ui/`)
- **UiManager**: GUI lifecycle and event management
- **InstallerGui**: Installation progress and user interaction

### 6. Service Management (`src/service/`)
- **ServiceManager**: Autorun configuration
- **Init Systems**: systemd, SysVinit, and BSD rc.d support

### 7. Monitoring (`src/monitoring.rs`)
- **Monitor**: Health checks and automatic recovery
- **Metrics**: Prometheus-compatible metrics export
- **Alerts**: Multi-severity alert system

## Data Flow

```
USB Boot → Live OS → Network Init → DHCP/mDNS
                  ↓
            Remote Access → VNC/SSH/WebVNC
                  ↓
            ISO Discovery → Mount → Installer Detection
                  ↓
            Disk Prep → Partition → Format
                  ↓
            OS Install → Progress Monitoring → Completion
```

## State Management

Each subsystem maintains its own state machine:
- **Network**: Down → Configuring → Up → Error → Recovering
- **ISO**: Idle → Scanning → Mounting → Ready → Installing
- **Disk**: Idle → Partitioning → Formatting → Busy → Error
- **Remote**: Stopped → Starting → Running → Stopping → Error

## Error Handling

- Layered error types with context propagation
- Automatic retry with exponential backoff
- Graceful degradation for non-critical failures
- Comprehensive logging with context

## Concurrency Model

- Tokio async runtime for I/O operations
- Arc<RwLock<T>> for shared state
- Channel-based communication between components
- Graceful shutdown coordination

## Security Considerations

- Root privileges required for disk/mount operations
- SSH key-based authentication by default
- Optional VNC password protection
- Self-signed certificates for HTTPS
- Credential redaction in logs