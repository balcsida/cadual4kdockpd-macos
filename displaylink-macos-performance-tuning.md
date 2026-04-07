# DisplayLink Hidden Settings & Architecture (macOS + Linux + Windows)

Undocumented preferences and internal architecture of DisplayLink drivers, reverse-engineered from macOS binary (v15.1.0), Linux driver (v6.2.0-30), and Windows driver (v12.2.1977.0). Key settings verified via Ghidra disassembly.

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

## Cross-Platform: XML Config Keys (`.dlua.xml` / `.dl.xml`)

These are **shared C++ core settings** used by all platforms. They live in a `DL_HIVE` XML config file, separate from `NSUserDefaults`.

**macOS location:** `~/Library/Group Containers/73YQY62QM3.com.displaylink.DisplayLinkShared/Library/Application Support/Logs/.dlua.xml`
**Linux location:** `/var/log/displaylink/.dl.xml`
**Windows:** Windows Registry under `HKEY_LOCAL_MACHINE\Software\DisplayLink\`

The XML path follows: `DL_HIVE > HKEY_LOCAL_MACHINE > Software > DisplayLink > ...`

### Confirmed present in macOS binary (potential tunables)

These keys were found in both the Linux `DisplayLinkManager` binary AND the macOS `DisplayLinkUserAgent` binary, confirming the shared C++ core reads them:

| Key | XML Path (under `DisplayLink/`) | Description | Potential impact |
|---|---|---|---|
| `ExtraModes` | `DisplayLinkManager/Configurations/<monitor>/Viewer/NivoSettings/ExtraModes` | Additional display modes beyond EDID | Could unlock more resolutions |
| `WindowSize` | `Core/WindowSize` | USB transfer window size | Larger = more throughput, more latency |
| `BandwidthAllocator` | `Core/BandwidthAllocator` | How USB bandwidth is split across displays | Could improve multi-display performance |
| `SessionStartDelay` | Connection auth path | Delay before HDCP session start | Lower = faster display activation |
| `LcTimeout` | HDCP locality check | Timeout for HDCP locality check | Faster display connection |
| `DeviceOverrides` | `DisplayLinkManager/DeviceOverrides` | Per-device config (next to `ConnectionTypes`, `NivoManager`) | Device-specific tuning |
| `MaskProtocolErrors` | USB transport layer | Mask/ignore protocol errors | Could reduce flickering from transient USB errors |
| `PnpSettings` | `DisplayLinkManager/PnpSettings` | Plug-and-play behavior config | |
| `PersistentDisplaysSettings` | `DisplayLinkManager/PersistentDisplaysSettings` | Persistent display configs | |
| `ContentHint` | Per-display `Viewer/ContentHint` | Content optimization hint | Only `Auto` confirmed on macOS; Linux has `Game`/`Cinema`/`Photo`/`Graphics` |

### Per-display Viewer settings (already in `.dlua.xml`)

These are per-monitor settings under each `Configurations/<monitor-serial>/Viewer/` node:

| Key | Current value | Other known values |
|---|---|---|
| `Type` | `NivoViewer` | — |
| `Rotation` | `0` | 0, 90, 180, 270 |
| `ContentHint` | `Auto` | `Game`, `Cinema`, `Photo`, `Graphics` (Linux-only?) |
| `AdjustedDisplayMode/AdjustedMode` | `0x0x0@0` | `<width>x<height>x<depth>@<freq>` |
| `AntiAutoAdjustingBorder/Colour` | `0` | Border color |
| `AntiAutoAdjustingBorder/Thickness_H` | `0` | Horizontal border thickness |
| `AntiAutoAdjustingBorder/Thickness_V` | `0` | Vertical border thickness |

### Logger configuration (can be added to `.dlua.xml`)

Based on the Linux `DLSupportTool.sh --debug` implementation, this XML structure enables debug logging. **Untested on macOS** — the encrypted `.log` files suggest macOS may encrypt output, but this config might enable plaintext logging:

```xml
<!-- Add under HKEY_LOCAL_MACHINE/Software/DisplayLink/ -->
<node name="DisplayLinkManager">
    <node name="Logger">
        <node name="Modules">
            <node name="connBandwidth"><value name="verbosity">WARN</value></node>
            <node name="default">
                <node name="Outputs">
                    <node name="Log"><value name="verbosity">DEBUG</value></node>
                </node>
            </node>
            <node name="UpdateEngineStats"><value name="verbosity">WARN</value></node>
            <node name="UpdateEngineThread"><value name="verbosity">WARN</value></node>
            <node name="DeviceWindow"><value name="verbosity">WARN</value></node>
        </node>
        <node name="Outputs">
            <node name="Log">
                <value name="filename">/tmp/DisplayLinkManager.log</value>
                <value name="line type">1</value>
                <value name="max size">500000</value>
                <value name="Mode">Decorate</value>
                <value name="maxFileCount">5</value>
            </node>
        </node>
    </node>
</node>

<!-- Add under HKEY_LOCAL_MACHINE/Software/DisplayLink/ -->
<node name="Logger">
    <value name="verbosity">DEBUG</value>
</node>
<node name="Core">
    <value name="EnableUniqueDmpFiles">Yes</value>
</node>
```

### Linux-only keys (NOT in macOS binary)

| Key | Description | Why it matters |
|---|---|---|
| `RenderRate` | Controls render rate in the update engine | Could control FPS at the encoder level |
| `showFPS` | Displays FPS counter | Invaluable for debugging performance |
| `FirmwareTrace` | Firmware-level trace logging | Debug dock firmware issues |
| `Scramble` | HDCP scrambling control | |
| `Powerups` | Power-on sequence control | |

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

### EVDI Library API — Bandwidth Control

The `evdi_connect2()` function reveals bandwidth control parameters:

```c
void evdi_connect2(evdi_handle handle, const unsigned char *edid,
    const unsigned int edid_length,
    const uint32_t pixel_area_limit,       // max pixels per frame
    const uint32_t pixel_per_second_limit); // max pixel throughput
```

These `pixel_area_limit` and `pixel_per_second_limit` parameters control quality/bandwidth tradeoffs at the kernel level.

### Linux: XML Config & Advanced Logging

```bash
# Enable advanced logging (requires xmlstarlet)
sudo DLSupportTool.sh --debug

# Disable advanced logging
sudo DLSupportTool.sh --nodebug
```

Logger modules configurable in `/var/log/displaylink/.dl.xml`:

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

---

## Windows: Network Adapter Properties (v12.2.1977.0)

The Windows CDC NCM driver (`dlcdcncm.inf`) exposes these configurable properties in **Device Manager > Network Adapter > Properties > Advanced**:

| Property | Type | Default | Description |
|---|---|---|---|
| `NetworkAddress` | edit (12 hex chars) | empty | **MAC address cloning** — enter e.g. `AABBCCDDEEFF` |
| `VlanId` | long (0-4094) | 0 | VLAN tagging |
| `*PriorityVLANTag` | enum | 3 (both enabled) | Priority & VLAN enable |
| `*SpeedDuplex` | enum | 0 (auto) | Speed: Auto / 10M / 100M / 1G Full Duplex |
| `*WakeOnMagicPacket` | enum | enabled | Wake on LAN |
| `*ModernStandbyWoLMagicPacket` | enum | enabled | Modern Standby WoL |
| `WakeOnLinkeStateChangeSupportedByHardware` | enum | enabled | Wake on link change |
| `*SelectiveSuspend` | enum | enabled | USB selective suspend |
| `*SSIdleTimeout` | long (1-60) | 5 sec | Selective suspend idle timeout |
| `*SSIdleTimeoutScreenOff` | long (1-60) | 3 sec | SS timeout when screen off |
| `*DeviceSleepOnDisconnect` | enum | enabled | Sleep on cable disconnect |
| `*EEE` | enum | disabled | Green Ethernet (802.3az) |

The driver also includes `BIOSMAC_VIDPID` entries (`0x17E94312`, `0x17E94318`) for reading MAC from BIOS/SMBIOS on specific devices.

### Windows: Default New Device Behavior

```
HKEY_LOCAL_MACHINE\software\DisplayLink\Products\DisplayLink Graphics\OnNewDevice
  Activity = "Extend"
  ExtendPosition = "Right"
  Launch = "WDP"
```

### Windows: Product Features

```
HKEY_LOCAL_MACHINE\Software\DisplayLink\Products\DisplayLink Graphics\Features
  HdmiOverscan = "Yes"
  ProtectedEnvironment = "No"
  CompatibilityCheck = "Yes"
  PromoteStoreApp = "Yes"
  BundleVmmFirmware = "No"
  WirelessProduct = "No"
```

### Windows: Supported Devices

The `ImportSettings.reg` contains product branding for 60+ devices from Kensington, IOGEAR, Samsung, Toshiba, LG, HP, Lenovo, Fujitsu, ASUS, Belkin, Dell, Anker, Plugable, Wacom, and Synaptics.

---

## Cross-Platform: Firmware

### Bundled firmware packages (in all three platforms)

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

## Experiments Completed (April 2026)

| Experiment | Result | Details |
|---|---|---|
| `ForceScreenRefresh=true` | **SUCCESS** | Disables GPU frame diffing. Noticeably smoother. Confirmed via Ghidra: skips `GpuDiffer` creation, every frame sent as full-screen update. **Recommended setting.** |
| `ForceBackendAPI=ScreenCaptureKit` | **No effect** | v15.1.0 already uses ScreenCaptureKit natively. Stack trace sampling confirmed both modes use `SCStream` with zero `CGDisplayStream` calls. |
| Debug logging via `.dlua.xml` Logger XML | **Failed** | macOS binary ignores Logger XML config entirely. Logs remain encrypted. The Linux `DLSupportTool.sh --debug` XML structure is not read by the macOS driver. |
| `ContentHint=Graphics` in `.dlua.xml` | **No perceptible effect** | XML was accepted and survived restart, but no visible difference — likely because `ForceScreenRefresh` already sends full uncompressed frames, making encoder hints irrelevant. |
| `DisableFluxGammaSupport=true` | **Applied** | Removes f.lux hook overhead. Minor/unmeasurable impact. |
| `DisableNightShiftSupport=true` | **Applied** | Skips CoreBrightness.framework loading. Minor/unmeasurable impact. |
| XML tunables without `ForceScreenRefresh` | **Made things worse** | Tested `WindowSize=65536`, `BandwidthAllocator/WindowSize=65536`, `ContentHint=Graphics`, `ExtraModes=1`, `MaskProtocolErrors=1` all together with `ForceScreenRefresh=false`. Result: "brutal latency." These XML tunables cannot substitute for `ForceScreenRefresh` and may actually degrade performance when the GPU differ is active. |

## Conclusion

For macOS on Apple Silicon, **the single most impactful setting is:**

```bash
defaults write com.displaylink.DisplayLinkUserAgent ForceScreenRefresh -bool true
killall DisplayLinkUserAgent && sleep 2 && open "/Applications/DisplayLink Manager.app"
```

This disables the Metal GPU frame differ, forcing every captured frame to be sent as a full-screen update. The ~3-5% extra CPU usage is negligible on M-series chips and eliminates perceived choppiness from skipped/partially-updated frames.

The XML-based tunables (`WindowSize`, `BandwidthAllocator`, `ContentHint`, `ExtraModes`, `MaskProtocolErrors`) in `.dlua.xml` were tested but provide no benefit and can make latency worse. **Do not use them as alternatives to `ForceScreenRefresh`.**

---

## DisplayLink Official GitHub Repos

Analysis of [github.com/DisplayLink](https://github.com/DisplayLink) repositories:

### EVDI — Frame Pacing Bug ([#512](https://github.com/DisplayLink/evdi/issues/512), [#562](https://github.com/DisplayLink/evdi/pull/562))

The EVDI issue tracker contains the **root cause explanation** for the same choppiness problem that `ForceScreenRefresh` works around on macOS.

[Issue #512](https://github.com/DisplayLink/evdi/issues/512) describes how the EVDI vblank timing is fundamentally broken:

> *"evdi fires vblank immediately after the user-space driver called `DRM_EVDI_GRABPIX`... This in turn will generally result in the drm master immediately starting to render a new frame, thinking the display is fast enough to want it, which in turn leads to new update ready events... The result is wasted CPU/GPU on useless frames that get thrown away anyway."*

[PR #562](https://github.com/DisplayLink/evdi/pull/562) fixes this on Linux by implementing **proper hrtimer-based vblank** at the display's actual refresh rate — the same pattern used by the `vkms` virtual display driver. This eliminates tearing and properly paces frames.

**On macOS**, the analogous problem manifests differently: the `GpuDiffer` (Metal-based frame differ) sometimes incorrectly determines frames are "unchanged" and skips them, or the frame delivery pipeline has poor timing causing frames to arrive in bursts rather than evenly paced. `ForceScreenRefresh=true` brute-forces past this by disabling the differ entirely and sending every frame as a full-screen update.

### DL-7450 SDK — Programmable Docks

The [DL-7450 repo](https://github.com/DisplayLink/dl-7450) is an SDK for programming the dock's embedded MicroPython processor (DL-7450 chipset only). It exposes:

- `DockInfo().monitors()` — query connected monitors, names, preferred modes
- `DockControl().suspend_host_connection(bool)` — programmatically suspend/resume USB connection
- `Splashscreen` — draw custom splash screens on the dock's displays
- `usb.find()` — enumerate USB devices connected to the dock
- GPIO, I2C, HTTP, MQTT, timers — full IoT capabilities

This SDK is for the **DL-7450 chip** (newer generation), not applicable to the USB-C Dual-4K Dock tested here, but demonstrates that DisplayLink docks can be programmed at the firmware level.

### Asahi Linux on Apple Silicon ([#559](https://github.com/DisplayLink/evdi/pull/559))

The EVDI kernel module has been patched to compile on **Asahi Linux (Fedora on Apple Silicon M1 Pro)**. This means the Linux DisplayLink driver works on Apple Silicon via Asahi Linux as an alternative to the macOS driver — using EVDI + DRM instead of ScreenCaptureKit, which is architecturally very different.

### Other Notable Issues

- [#552](https://github.com/DisplayLink/evdi/issues/552) — Low refresh rate (below 20 FPS visually despite 60 Hz reported) after EVDI update. Same symptom class as the macOS choppiness.
- [#522](https://github.com/DisplayLink/evdi/issues/522) — DisplayLinkManager consuming 14% CPU with no monitors connected. Related to the update loop spinning without proper throttling.
- [#548](https://github.com/DisplayLink/evdi/pull/548) — CPU cache flush fix for cache coherency. Performance-relevant for the frame buffer pipeline.

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
- **EVDI library API**: From `evdi_lib.h` — `evdi_connect2()` bandwidth parameters
- **EVDI GitHub issues**: Frame pacing bug ([#512](https://github.com/DisplayLink/evdi/issues/512)), vblank fix ([#562](https://github.com/DisplayLink/evdi/pull/562)), Asahi Linux support ([#559](https://github.com/DisplayLink/evdi/pull/559))
- **Cross-platform XML keys**: Found by cross-referencing strings in Linux `DisplayLinkManager` (aarch64) and macOS `DisplayLinkUserAgent` (arm64)
- **ContentHint values**: From Linux `DisplayLinkManager` binary strings (confirmed macOS-absent via Ghidra)
- **Windows driver**: Extracted from PE32 installer → 7z SFX → driver packages. Registry settings from embedded `ImportSettings.reg` and `dlcdcncm.inf`
- **DL-7450 SDK**: From [github.com/DisplayLink/dl-7450](https://github.com/DisplayLink/dl-7450) — MicroPython dock programming interface

## Notes

- Hidden preferences may change or disappear with any DisplayLink update
- DisplayLink on macOS works via screen capture (not native GPU output) — a fundamental Apple limitation
- The Linux driver uses EVDI (virtual display interface in kernel) + DRM, which is architecturally different from macOS
- Windows uses native GPU driver integration (WDDM), not screen capture — fundamentally different from both macOS and Linux
- The frame pacing issue ([EVDI #512](https://github.com/DisplayLink/evdi/issues/512)) is the root cause of choppiness across platforms; `ForceScreenRefresh` on macOS and hrtimer vblank ([EVDI #562](https://github.com/DisplayLink/evdi/pull/562)) on Linux are different solutions to the same underlying problem
- Tested April 2026. YMMV on different versions
