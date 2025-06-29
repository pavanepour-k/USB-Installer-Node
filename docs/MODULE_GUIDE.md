# Module Guide

## Core Modules

### `main.rs`
Entry point orchestrating initialization, subsystem startup, and graceful shutdown.

**Key Functions:**
- `AppState::new()` - Initialize all managers
- `AppState::initialize()` - Check preconditions and start monitoring
- `AppState::run()` - Main event loop with signal handling
- `AppState::shutdown()` - Coordinated subsystem shutdown

### `config.rs`
Configuration management with validation and hot-reloading.

**Types:**
- `Config` - Root configuration structure
- `NetworkConfig`, `RemoteConfig`, `IsoConfig`, etc. - Subsystem configs
- `ConfigManager` - Runtime configuration updates

### `error.rs`
Centralized error handling with context propagation.

**Error Types:**
- `ConfigError` - Configuration parsing/validation
- `NetworkError` - Network operations
- `DiskError` - Disk management
- `IsoError` - ISO operations
- `RemoteError` - Remote access
- `ServiceError` - Service management
- `UiError` - GUI operations
- `MonitoringError` - Health monitoring

## Network Module (`network/`)

### `network.rs`
High-level network orchestration.

**Components:**
- `NetworkManager` - Coordinates DHCP, hostname, and tunnel
- `NetworkState` - State machine implementation
- `NetworkStatus` - Current network information

### `dhcp.rs`
DHCP client implementation with retry logic.

**Features:**
- Interface auto-detection
- Lease acquisition/renewal
- Exponential backoff on failure

### `hostname.rs`
Hostname generation and mDNS registration.

**Features:**
- Random suffix generation
- Platform-specific hostname setting
- Avahi/mdnsd integration

### `tunnel.rs`
VPN tunnel management.

**Supported Providers:**
- Tailscale
- WireGuard
- SSH tunnels

## Disk Module (`disk/`)

### `disk.rs`
Disk operation orchestration.

**Features:**
- Auto-partitioning based on config
- Batch formatting operations
- Progress tracking

### `partition.rs`
Low-level partitioning operations.

**Features:**
- MBR/GPT support
- Partition CRUD operations
- Size calculation utilities

### `format.rs`
Filesystem formatting.

**Supported Filesystems:**
- ext4, ext3, ext2
- xfs, btrfs
- ntfs, vfat
- f2fs

## ISO Module (`iso/`)

### `iso.rs`
ISO lifecycle management.

**Features:**
- Directory scanning
- Auto-mounting
- State tracking

### `mounter.rs`
Loop device mounting.

**Features:**
- Mount state tracking
- Concurrent mount support
- Automatic cleanup

### `installer.rs`
OS installer detection and execution.

**Supported OS Types:**
- Debian/Ubuntu
- Windows
- BSD variants

## Remote Module (`remote/`)

### `remote.rs`
Unified remote access management.

**Features:**
- Service orchestration
- Health monitoring
- Dynamic reconfiguration

### `vnc.rs`
X11VNC server management.

**Features:**
- Process lifecycle management
- Client tracking
- Automatic restart on crash

### `ssh.rs`
OpenSSH server management.

**Features:**
- Key generation
- Authorization management
- Session tracking

### `web_vnc.rs`
NoVNC web interface.

**Features:**
- WebSocket proxy
- HTTPS support
- Session management

## UI Module (`ui/`)

### `ui.rs`
UI orchestration and message routing.

**Features:**
- Event processing
- Progress updates
- Remote input handling

### `installer_gui.rs`
Installation GUI implementation.

**Features:**
- Progress visualization
- Log display
- Error handling

## Service Module (`service/`)

### `service.rs`
Service management wrapper.

### `init.rs`
Init system integration.

**Supported Systems:**
- systemd (Linux)
- SysVinit (Linux)
- rc.d (BSD)

## Supporting Modules

### `logging.rs`
Structured logging with rotation.

**Features:**
- Multi-target output
- Log rotation
- Context macros

### `monitoring.rs`
Health monitoring and metrics.

**Features:**
- Service health checks
- Automatic recovery
- Prometheus metrics

## Module Dependencies

```
main.rs
  ├── config.rs
  ├── error.rs
  ├── logging.rs
  ├── monitoring.rs
  ├── network/
  │   ├── dhcp.rs
  │   ├── hostname.rs
  │   └── tunnel.rs
  ├── disk/
  │   ├── partition.rs
  │   └── format.rs
  ├── iso/
  │   ├── mounter.rs
  │   └── installer.rs
  ├── remote/
  │   ├── vnc.rs
  │   ├── ssh.rs
  │   └── web_vnc.rs
  ├── ui/
  │   └── installer_gui.rs
  └── service/
      └── init.rs
```