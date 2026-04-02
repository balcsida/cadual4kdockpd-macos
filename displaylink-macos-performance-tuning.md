# DisplayLink Hidden Settings & Architecture (macOS + Linux)

Undocumented preferences and internal architecture of DisplayLink drivers, reverse-engineered from macOS binary (v15.1.0) and Linux driver (v6.2.0-30).

Tested on: **Apple M3 Max, macOS Tahoe 26.4, DisplayLink Manager 15.1.0, DisplayLink USB-C Dual-4K Dock (GenesysLogic GL352x hub, FW 0x9305), 2x HP E273 (1080p @ 60Hz)**

---

## macOS: The Key Setting — `ForceScreenRefresh`

This is the single most impactful setting for perceived smoothness on DisplayLink displays:

```bash
defaults write com.displaylink.DisplayLinkUserAgent ForceScreenRefresh -bool true
```

Forces DisplayLink to capture fresh frames more aggressively rather than skipping frames it considers unchanged. Results in noticeably smoother window dragging and scrolling on DisplayLink-connected monitors.

**To revert:**

```bash
defaults delete com.displaylink.DisplayLinkUserAgent ForceScreenRefresh
```

After changing, restart DisplayLink:

```bash
killall DisplayLinkUserAgent && sleep 2 && open "/Applications/DisplayLink Manager.app"
```

## macOS: All Hidden `defaults write` Keys

Complete list of NSUserDefaults keys found in the `DisplayLinkUserAgent` binary:

### Performance & Rendering

| Key | Type | Description |
|---|---|---|
| `ForceScreenRefresh` | Bool | **Best perf tweak** — forces aggressive frame capture instead of skipping unchanged frames |
| `ForceBackendAPI` | String | Accepts `"ScreenCaptureKit"`. On v15.1.0, the app already uses ScreenCaptureKit by default (confirmed via stack trace sampling with `sample`), so this may not change behavior |
| `FallbackBackendAPI` | String | Fallback capture API if the primary fails |
| `DisableGpuDiffing` | Bool | Disables Metal-based GPU frame diffing. **Don't set this** — GPU diffing is beneficial |
| `DetectVideoOverlay` | Bool | Auto-detect video call overlays — **known performance issue** when enabled (causes CPU spikes and lag, see [BetterDisplay #4905](https://github.com/waydabber/BetterDisplay/discussions/4905)) |

### Display Modes

| Key | Type | Description |
|---|---|---|
| `EnableExperimentalModes` | Bool | Enables experimental display modes. [Officially documented for HiDPI](https://support.displaylink.com/knowledgebase/articles/1993915-displaylink-manager-app-experimental-hidpi-modes) |
| `EnableLargeHiDPIModes` | Bool | Enables scaled 3008x, 2560x and more modes |

### Color Processing

| Key | Type | Description |
|---|---|---|
| `DisableFluxGammaSupport` | Bool | Skips f.lux color processing integration |
| `DisableNightShiftSupport` | Bool | Skips Night Shift color temperature processing |

### Application Behavior

| Key | Type | Description |
|---|---|---|
| `ShowMenuBarIcon` | Bool | Hide/show the menu bar icon (`0` to hide, `1` to show) |
| `AppAutostart` | Bool | Auto-launch DisplayLink Manager on login |
| `DisableRestartOnCrash` | Bool | Prevents auto-restart after a crash |

## macOS: Support Tool Utilities

The **DisplayLink Support Tool** (`/Applications/DisplayLink Support Tool.app`) bundles several useful utilities:

### `DisplayDetails` binary

Dumps display IDs, modes, positions, and flags:

```bash
"/Applications/DisplayLink Support Tool.app/Contents/MacOS/DisplayDetails"
```

Example output:
```
Online displays count: 3
Display ID: 0x1 Flags: ["MAIN", "BUILT-IN"]
Mode: 3456x2234@1.2e+02 (PHYSICAL), 1728x1117 (VIRTUAL)
Position: (0.0, 0.0) Rotation: 0.0

Display ID: 0x3f Flags: []
Mode: 1920x1080@60
Position: (-1079.0, -1080.0) Rotation: 0.0
```

### `ChangeLoggingLevel` binary

Sets logging verbosity (Debug / Info / Verbose) by editing `.dlua.xml`:

```bash
"/Applications/DisplayLink Support Tool.app/Contents/MacOS/ChangeLoggingLevel"
```

### `dlinfo` script

Collects a comprehensive system diagnostics zip including IORegistry dumps, WindowServer prefs, crash reports, DisplayLink logs, and EDID files.

### `ResetDisplayPrefs.command` script

Nuclear option — deletes all display layout preferences (WindowServer plists and DisplayLink shared prefs). Requires reboot.

## macOS: XPC Service (DDC Monitor Control)

The `DisplayLinkXpcService` binary exposes DDC monitor control:

- **Brightness** — set/get per-display
- **Contrast** — set/get per-display
- **Flip/Rotation** — per-display
- **Display enable/disable** — toggle individual displays

---

## Linux: Driver Architecture (v6.2.0)

The Linux driver (`displaylink-driver-6.2.0-30.run`) consists of:

- **EVDI** — open-source kernel module (Extensible Virtual Display Interface, v1.14.11), [source on GitHub](https://github.com/DisplayLink/evdi)
- **DisplayLinkManager** — closed-source userspace daemon (runs as root from `/opt/displaylink/`)
- **libevdi** — userspace library for communicating with the kernel module
- **Firmware packages** — same `.spkg` files as macOS (ella, navarro, ridge, firefly)

### EVDI Kernel Module Parameters

```bash
# Load-time parameters (via /etc/modprobe.d/evdi.conf)
modprobe evdi initial_loglevel=4 initial_device_count=0
```

| Parameter | Type | Default | Description |
|---|---|---|---|
| `initial_loglevel` | int | 4 (INFO) | 1=FATAL, 2=ERROR, 3=WARN, 4=INFO, 5=DEBUG, 6=VERBOSE |
| `initial_device_count` | ushort | 0 | Pre-create DRM devices at load time |

### EVDI Sysfs Interface

Runtime-writable sysfs attributes at `/sys/devices/evdi/`:

| Attribute | Access | Description |
|---|---|---|
| `version` | RO | EVDI module version |
| `count` | RO | Number of active EVDI devices |
| `loglevel` | RW | **Runtime log level** (0-6, writable without reload!) |
| `add` | WO | Create new virtual display (write device count or `usb:<path>`) |
| `remove_all` | WO | Remove all virtual displays |

### Linux: XML Config & Advanced Logging

The `DLSupportTool.sh` script reveals the `.dl.xml` configuration structure and logging system:

```bash
# Enable advanced logging (requires xmlstarlet)
sudo DLSupportTool.sh --debug

# Disable advanced logging
sudo DLSupportTool.sh --nodebug
```

The `--debug` flag configures these logger modules in `/var/log/displaylink/.dl.xml`:

| Module | Default verbosity | Description |
|---|---|---|
| `connBandwidth` | WARN | Connection bandwidth monitoring |
| `default` | DEBUG (to Log output) | General logging |
| `UpdateEngineStats` | WARN | Frame update statistics |
| `UpdateEngineThread` | WARN | Update engine thread activity |
| `DeviceWindow` | WARN | Device window management |
| `LUsbConnAlex` | WARN | USB connection (Alex chipset) |
| `LUsbConnFpga` | WARN | USB connection (FPGA chipset) |

Log output configuration:
```xml
<node name="Log">
  <value name="filename">/var/log/displaylink/DisplayLinkManager.log</value>
  <value name="line type">1</value>
  <value name="max size">500000</value>
  <value name="Mode">Decorate</value>
</node>
```

### Linux: Suspend/Resume IPC

DisplayLinkManager uses named pipes for power management:
- `/tmp/PmMessagesPort_in` — write `S` to suspend, `R` to resume
- `/tmp/PmMessagesPort_out` — read result after suspend

### Linux: USB Power Management

The udev bootstrap script disables USB3 Link Power Management (U1/U2) for DisplayLink devices:
```bash
echo 0 > /sys/$DEVPATH/../port/usb3_lpm_permit
```
This prevents random disconnections caused by aggressive USB power saving.

---

## Cross-Platform: ContentHint Values

From the Linux binary, the valid `ContentHint` values for per-display optimization are:

| Value | Description |
|---|---|
| `Auto` | Default — automatic detection |
| `Game` | Optimized for gaming content |
| `Cinema` | Optimized for video playback |
| `Photo` | Optimized for still images |
| `Graphics` | Optimized for desktop/graphics work |

These map to the `ContentHint` field in `.dlua.xml` (macOS) or `.dl.xml` (Linux) per-display configuration.

## Cross-Platform: Firmware

### Bundled firmware packages (in both macOS and Linux drivers)

| File | Codename | Type | Size (Linux) |
|---|---|---|---|
| `ella-dock-release.spkg` | EllaDockOW | Dock | 925 KB |
| `navarro-dock-release.spkg` | NavaDockOW | Dock | 1.7 MB |
| `ridge-dock-release.spkg` | — | Dock | 883 KB |
| `firefly-monitor-release.spkg` | — | Monitor | 363 KB |

All use the "ELLA" container format (Synaptics internal). DL-7xxx/6xxx/5xxx/3xxx/41xx devices are automatically upgraded upon connection on Linux.

### USB Hub Firmware (GenesysLogic GL352x)

The dock's USB hub uses a **GenesysLogic GL352x** chip (VID `0x05E3`, PID `0x0610`). A separate Windows-only firmware updater (`CADUAL4KDOCKPD_9392.exe` / "USB3.0 Hub Update Tool v3.001") can update the hub firmware from version 9311 → 9392, fixing random disconnection issues. Uses `GL_SS_HUB_ISP.exe` internally, flashing `Hub_1_9392_L1.ROM` and `Hub_1_9392_L2.ROM`.

Current hub firmware can be read from `bcdDevice` in IORegistry (macOS) or `/sys/bus/usb/devices/*/bcdDevice` (Linux). Example: decimal `37637` = hex `0x9305`.

---

## Internal Rendering Pipeline (macOS)

From binary analysis:

1. **Screen capture** → ScreenCaptureKit (`SCStream`, `SCContentFilter`, `SCShareableContent`)
2. **GPU diffing** → Metal shaders (`GpuDiffer`, `ShaderLibrary.metallib`) to detect changed regions
3. **Zero-copy encoding** → `ZeroCopyGpuEncodeSurfacesWrapper` for efficient frame encoding
4. **Healing frames** → `HealingFramesWatchdog` periodically sends full frames to fix corruption
5. **Bandwidth allocation** → `BandwidthAllocator` manages throughput across multiple displays
6. **Rendering** → `DLRenderPipeline` / `DLRenderEncoder` using Metal render pipeline
7. **NivoViewer** → Per-display viewer with `ContentHint`, rotation, border adjustment

### Log & Config Locations (macOS)

| Path | Description |
|---|---|
| `~/Library/Group Containers/73YQY62QM3.com.displaylink.DisplayLinkShared/Library/Application Support/Logs/` | Logs, EDID files, config XML |
| `*.log` | Application logs (encrypted/obfuscated) |
| `.dlua.xml` | User agent config (logging level, display configurations) |
| `*.edid` | Raw EDID data per monitor (256 bytes each) |
| `dlm_message_server_in#1` / `dlm_message_server_out#1` | Named pipes for IPC |

---

## Methodology

- macOS `defaults write` keys extracted from binary strings correlated with `NSUserDefaults` access patterns (`boolForKey:`, `stringForKey:`)
- ScreenCaptureKit backend verified via `sample` (call stack sampling) — both default and forced modes showed `SCStream` calls, zero `CGDisplayStream` calls
- Linux driver extracted from Makeself archive (`tail -n +714 | gzip -cd | tar xf -`)
- EVDI parameters from kernel module source code (`evdi_params.c`, `evdi_sysfs.c`)
- ContentHint values from Linux `DisplayLinkManager` binary strings
- Logger modules from `DLSupportTool.sh` XML editing functions
- Support Tool scripts read directly as plaintext shell/bash

For deeper analysis of the stripped macOS binary (e.g., `ForceScreenRefresh` capture loop behavior, `BandwidthAllocator` tuning, `ZeroCopyGpuEncodeSurfacesWrapper` internals), Ghidra disassembly would be needed.

## Notes

- Hidden preferences may change or disappear with any DisplayLink update
- DisplayLink on macOS works via screen capture (not native GPU output) — a fundamental Apple limitation
- The Linux driver uses EVDI (virtual display interface in kernel) + DRM, which is architecturally different from macOS
- Tested April 2026. YMMV on different versions
