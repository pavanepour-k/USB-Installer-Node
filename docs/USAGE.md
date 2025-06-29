# USB Installer Node Usage Guide

## Prerequisites

- Linux or BSD operating system
- Root/sudo privileges
- Required packages:
  ```bash
  # Debian/Ubuntu
  apt install build-essential x11vnc openssh-server parted dosfstools ntfs-3g websockify novnc

  # FreeBSD
  pkg install rust x11vnc openssh parted e2fsprogs ntfsprogs websockify novnc
  ```

## Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/pavanepour-k/USB-Installer-Node.git
   cd USB-Installer-Node
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Install the binary:
   ```bash
   sudo cp target/release/usb-installer-node /usr/local/bin/
   ```

## Configuration

Create `/etc/usb-installer-node/config.toml`:

```toml
[logging]
level = "info"
file_path = "/var/log/usb-installer-node.log"
console = true
max_file_size = 10485760
max_files = 5

[network]
interface = "auto"  # or specify "eth0"
dhcp_timeout = 30
hostname_prefix = "usb-node"
mdns_enabled = true

[network.tunnel]
enabled = false
provider = "tailscale"
reconnect_interval = 60

[remote.vnc]
enabled = true
port = 5900
display = ":0"
allow_shared = true
view_only = false

[remote.ssh]
enabled = true
port = 22
host_key_path = "/etc/ssh/ssh_host_rsa_key"
authorized_keys_path = "/root/.ssh/authorized_keys"
allow_password_auth = false

[remote.web_vnc]
enabled = true
listen_port = 6080
vnc_host = "localhost"
vnc_port = 5900
enable_auth = false

[iso]
enabled = true
iso_paths = ["/installers", "/media/usb"]
patterns = ["*.iso"]
mount_point = "/mnt/iso"
auto_scan = true
auto_mount = true
auto_launch = false

[ui]
enabled = true
theme = "dark"
language = "en"
fullscreen = false
show_logs = true

[disk]
enabled = true
auto_partition = false
auto_format = false

[service]
autorun = true
service_name = "usb-installer-node"

[monitoring]
enabled = true
check_interval = 30
max_failures = 3
auto_restart = true
```

## Creating USB Installer

1. Prepare USB drive (minimum 4GB):
   ```bash
   # List available disks
   sudo fdisk -l

   # Create bootable USB (replace /dev/sdX with your USB device)
   sudo dd if=debian-live.iso of=/dev/sdX bs=4M status=progress
   ```

2. Mount USB and add installer:
   ```bash
   sudo mkdir -p /mnt/usb
   sudo mount /dev/sdX1 /mnt/usb
   sudo cp /usr/local/bin/usb-installer-node /mnt/usb/
   sudo cp -r /etc/usb-installer-node /mnt/usb/
   ```

3. Add OS installers:
   ```bash
   sudo mkdir -p /mnt/usb/installers
   sudo cp debian-*.iso ubuntu-*.iso /mnt/usb/installers/
   ```

## Usage

### Manual Start
```bash
sudo usb-installer-node
```

### Service Mode
```bash
# Enable autostart
sudo systemctl enable usb-installer-node

# Start service
sudo systemctl start usb-installer-node

# Check status
sudo systemctl status usb-installer-node
```

### Remote Access

1. **VNC Access:**
   ```bash
   vncviewer <target-ip>:5900
   ```

2. **SSH Access:**
   ```bash
   ssh root@<target-ip>
   ```

3. **Web Access:**
   ```
   http://<target-ip>:6080/vnc.html
   ```

### Environment Variables

- `USB_INSTALLER_LOG_LEVEL` - Override log level
- `USB_INSTALLER_INTERFACE` - Override network interface
- `USB_INSTALLER_VNC_PORT` - Override VNC port
- `USB_INSTALLER_SSH_PORT` - Override SSH port

## Monitoring

### View Logs
```bash
tail -f /var/log/usb-installer-node.log
```

### Metrics Endpoint
```
http://<target-ip>:9090/metrics
```

### Health Check
```bash
curl http://<target-ip>:9090/health
```

## Troubleshooting

### Network Issues
- Check interface status: `ip link show`
- Verify DHCP: `journalctl -u usb-installer-node | grep dhcp`
- Test connectivity: `ping -c 4 8.8.8.8`

### Remote Access Issues
- VNC: Check X server: `ps aux | grep X`
- SSH: Verify keys: `ls -la /root/.ssh/`
- WebVNC: Check websockify: `ps aux | grep websockify`

### ISO Issues
- List mounted ISOs: `mount | grep loop`
- Check ISO detection: `ls -la /installers/`
- Verify mount point: `ls -la /mnt/iso/`

### Service Issues
- Check service logs: `journalctl -u usb-installer-node -f`
- Restart service: `systemctl restart usb-installer-node`
- Disable autostart: `systemctl disable usb-installer-node`