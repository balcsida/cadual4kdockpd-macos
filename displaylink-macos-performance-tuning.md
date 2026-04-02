# DisplayLink Hidden Settings & Architecture (macOS + Linux)

Undocumented preferences and internal architecture of DisplayLink drivers, reverse-engineered from macOS binary (v15.1.0) and Linux driver (v6.2.0-30). Key settings verified via Ghidra disassembly.

Tested on: **Apple M3 Max, macOS Tahoe 26.4, DisplayLink Manager 15.1.0, DisplayLink USB-C Dual-4K Dock (GenesysLogic GL352x hub, FW 0x9305), 2x HP E273 (1080p @ 60Hz)**

---

## macOS: The Key Setting — `ForceScreenRefresh`

This is the single most impactful setting for perceived smoothness on DisplayLink displays:

```bash
defaults write com.displaylink.DisplayLinkUserAgent ForceScreenRefresh -bool true
```

**To revert:**

```bash
defaults delete com.displaylink.DisplayLinkUserAgent ForceScreenRefresh
```

After changing, restart DisplayLink:

```bash
killall DisplayLinkUserAgent && sleep 2 && open "/Applications/DisplayLink Manager.app"
```

### How It Works (Ghidra-verified)

The `BaseScreenRecorder` class has a boolean field `nextFrameNeedsFullScreenUpdate`, initialized to `true`. When a new IOSurface frame arrives from screen capture, the frame processing function (`FUN_100050310`) follows this logic:

1. A new frame arrives from `SCStream` (ScreenCaptureKit)
2. Check if a `GpuDiffer` (Metal-based frame differ) exists
3. **If no differ exists**, read `ForceScreenRefresh` via `NSUserDefaults.boolForKey:`
4. If `ForceScreenRefresh` is **false** (default): create a `GpuDiffer` to detect changed screen regions. Only changed regions are sent to hardware, saving bandwidth but occasionally skipping frames the differ considers "unchanged"
5. If `ForceScreenRefresh` is **true**: **skip differ creation entirely**. `nextFrameNeedsFullScreenUpdate` stays `true`, so every captured frame is treated as a full-screen update

**This is why it feels smoother**: without GPU diffing, every captured frame is sent in its entirety to the DisplayLink hardware. There is zero chance of the differ incorrectly marking a frame as "unchanged" and skipping it. The tradeoff is more CPU/USB bandwidth usage — but on an M3 Max with 1080p displays, this overhead is negligible.

> **Note:** `ForceScreenRefresh` is only read at startup via `registerDefaults:` and during frame processing. It is **not** KVO-observed at runtime, so changes require an app restart.

---

## macOS: Screen Recorder Architecture (Ghidra-verified)

DisplayLink implements **three screen capture backends** plus a runtime switcher:

| Class | Backend API | Role |
|---|---|---|
| `SCKitScreenRecorder` | ScreenCaptureKit (`SCStream`, `SCContentFilter`) | Primary on macOS 12.3+ |
| `CGDSScreenRecorder` | `CGDisplayStreamCreateWithDispatchQueue` | Legacy fallback |
| `AVFoundationScreenRecorder` | `AVCaptureVideoDataOutput` | Oldest fallback |
| `SwitchingScreenRecorder` | Wraps primary + fallback | Runtime backend switching |

`SwitchingScreenRecorder::init` takes a **primary** and **fallback** recorder. It maintains a dispatch queue, tracks the current active backend, and stores the last gamma ramp event and display data for seamless switching.

### Frame Rate Configuration (Ghidra-verified)

The `SCStreamConfiguration.minimumFrameInterval` is set by `FUN_10004b5b4`:

1. Iterates `NSScreen.screens`, looking for the screen matching the DisplayLink display
2. Calls `NSScreen.maximumFramesPerSecond` for that screen
3. If the screen's max FPS ≥ 61: clamps to **60 Hz** (i.e., `CMTime(1, 60)`)
4. If < 61: uses the screen's native rate
5. Hard cap at **120 Hz** maximum (`CMTime(1, 120)`)

The `SCStreamConfiguration` is created with these fixed properties:
- `pixelFormat`: `0x42475241` (ARGB/BGRA)
- `queueDepth`: 7
- `showsCursor`: true
- `scalesToFit`: false
- `capturesAudio`: false
- `excludesCurrentProcessAudio`: true

---

## macOS: All Hidden `defaults write` Keys

Complete list of NSUserDefaults keys found in the `DisplayLinkUserAgent` binary:

### Performance & Rendering

| Key | Type | Verified | Description |
|---|---|---|---|
| `ForceScreenRefresh` | Bool | Ghidra ✓ | **Best perf tweak** — disables GPU frame diffing, forces every frame as full-screen update |
| `ForceBackendAPI` | String | Ghidra ✓ | KVO-observed at runtime. Non-empty string = true. Likely controls which backend is primary in `SwitchingScreenRecorder` |
| `FallbackBackendAPI` | String | strings | Fallback capture API if the primary fails |
| `DisableGpuDiffing` | Bool | strings | May be an alias for `ForceScreenRefresh` or a separate code path |
| `DetectVideoOverlay` | Bool | Ghidra ✓ | Auto-detect video call overlays — **known performance issue** when enabled (causes CPU spikes and lag, see [BetterDisplay #4905](https://github.com/waydabber/BetterDisplay/discussions/4905)). Read in `UserSettings::init` |

### Display Modes

| Key | Type | Verified | Description |
|---|---|---|---|
| `EnableExperimentalModes` | Bool | strings | Enables experimental display modes. [Officially documented for HiDPI](https://support.displaylink.com/knowledgebase/articles/1993915-displaylink-manager-app-experimental-hidpi-modes) |
| `EnableLargeHiDPIModes` | Bool | Ghidra ✓ | Enables scaled 3008x, 2560x and more modes. Read in `UserSettings::init` |

### Color Processing

| Key | Type | Verified | Description |
|---|---|---|---|
| `DisableFluxGammaSupport` | Bool | strings | Skips f.lux color processing integration |
| `DisableNightShiftSupport` | Bool | Ghidra ✓ | When true, skips `dlopen` of `CoreBrightness.framework` entirely, preventing Night Shift hooks. The `NightShiftSingleton` is never initialized |

### Application Behavior

| Key | Type | Verified | Description |
|---|---|---|---|
| `ShowMenuBarIcon` | Bool | Ghidra ✓ | Hide/show the menu bar icon. Read directly from `UserSettings.showMenuBarIcon` field in `AppDelegate` init |
| `AppAutostart` | Bool | Ghidra ✓ | Auto-launch on login. KVO-observed at runtime alongside `ForceBackendAPI` |
| `DisableRestartOnCrash` | Bool | Ghidra ✓ | When false (default), registers an `SMAppService` agent plist for automatic crash recovery restart |

### KVO-Observed Keys (Runtime Changes Without Restart)

Only **two** keys are watched via `observeValueForKeyPath:`:
- `AppAutostart` (bool change triggers Combine `Published` update)
- `ForceBackendAPI` (string change → converted to bool: non-empty = true)

All other settings require an app restart to take effect.

---

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

Display ID: 0x3f Flags: []
Mode: 1920x1080@60
```

### `ChangeLoggingLevel` binary

Sets logging verbosity (Debug / Info / Verbose) by editing `.dlua.xml`.

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

The `DLSupportTool.sh` script reveals the `.dl.xml` configuration structure:

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

From the Linux `DisplayLinkManager` binary, the valid `ContentHint` values for per-display optimization are:

| Value | Description |
|---|---|
| `Auto` | Default — automatic detection |
| `Game` | Optimized for gaming content |
| `Cinema` | Optimized for video playback |
| `Photo` | Optimized for still images |
| `Graphics` | Optimized for desktop/graphics work |

> **macOS note:** Ghidra analysis confirms only `Auto` is referenced in the macOS binary. The Game/Cinema/Photo/Graphics values are **Linux-only** and not functional on macOS.

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

## Internal Rendering Pipeline (macOS, Ghidra-verified)

### Capture Layer

```
SCKitScreenRecorder
  └─ SCStream (ScreenCaptureKit)
       ├─ SCContentFilter (initWithDisplay:excludingApplications:exceptingWindows:)
       ├─ SCStreamConfiguration
       │    ├─ minimumFrameInterval: CMTime(1, displayFPS) — capped at 120Hz
       │    ├─ queueDepth: 7
       │    ├─ pixelFormat: BGRA
       │    └─ showsCursor: true
       └─ CaptureStreamOutput (receives IOSurface frames)
            └─ silentRestartsThreshold: read from NSUserDefaults integerForKey (default: 6)
```

### Processing Layer

```
BaseScreenRecorder (processes captured frames)
  ├─ nextFrameNeedsFullScreenUpdate: bool (init: true)
  ├─ gpuDifferCreationRequired: bool (init: false)
  ├─ differ: GpuDiffer? (Metal-based frame differ)
  ├─ framesWatchdog: monitors frame delivery
  ├─ healingFramesWatchdog: periodically forces full frames
  ├─ frameTransformer: applies transformations
  ├─ gammaApplier: color correction
  ├─ shadowSurfaces: double-buffered IOSurfaces
  └─ latestSurface: most recent captured IOSurface
```

### Frame Processing Flow (ForceScreenRefresh path)

```
New IOSurface arrives
  │
  ├─ differ exists? ──yes──▶ Use GpuDiffer to find changed rects
  │                           └─ Send only changed regions
  │
  └─ differ is nil?
       │
       ├─ ForceScreenRefresh=false ──▶ Create GpuDiffer (FUN_100047f84)
       │                                └─ Future frames use diff-based updates
       │
       └─ ForceScreenRefresh=true ──▶ Skip differ creation
                                       └─ Every frame = full screen update
                                            (nextFrameNeedsFullScreenUpdate stays true)
```

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

- **macOS `defaults write` keys**: Extracted from binary strings, correlated with `NSUserDefaults` access patterns (`boolForKey:`, `stringForKey:`, `integerForKey:`)
- **ScreenCaptureKit verification**: `sample` (call stack sampling) confirmed both default and forced modes use `SCStream`, zero `CGDisplayStream` calls
- **Ghidra disassembly** of `DisplayLinkUserAgent` (arm64 Mach-O, 13 MB, stripped):
  - `UserSettings::init` (`0x10002a410`): reads all defaults into Swift `@Published` properties
  - `UserSettings::observeValueForKeyPath` (`0x10002b538`): KVO handler for `AppAutostart` and `ForceBackendAPI`
  - `FUN_100050310`: frame processing function that reads `ForceScreenRefresh` and conditionally creates `GpuDiffer`
  - `FUN_10004b66c`: `SCStreamConfiguration` setup with frame interval, pixel format, queue depth
  - `FUN_10004b5b4`: frame interval calculation (CMTime from display FPS, clamped 60-120Hz)
  - `FUN_1000480f0`: `NightShiftSingleton` init that reads `DisableNightShiftSupport` and conditionally loads CoreBrightness
  - `FUN_10000adec`: `AppDelegate` init that reads `DisableRestartOnCrash` and registers `SMAppService`
- **Linux driver**: Extracted from Makeself archive (`tail -n +714 | gzip -cd | tar xf -`)
- **EVDI parameters**: From kernel module source code (`evdi_params.c`, `evdi_sysfs.c`)
- **ContentHint values**: From Linux `DisplayLinkManager` binary strings (confirmed macOS-absent via Ghidra)

## Notes

- Hidden preferences may change or disappear with any DisplayLink update
- DisplayLink on macOS works via screen capture (not native GPU output) — a fundamental Apple limitation
- The Linux driver uses EVDI (virtual display interface in kernel) + DRM, which is architecturally different from macOS
- Tested April 2026. YMMV on different versions
