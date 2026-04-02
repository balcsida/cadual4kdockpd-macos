# DisplayLink macOS Hidden Performance Settings

Undocumented `defaults write` preferences for DisplayLink Manager on macOS, reverse-engineered from the `DisplayLinkUserAgent` binary (v15.1.0).

Tested on: **Apple M3 Max, macOS Tahoe 26.4, DisplayLink Manager 15.1.0, DisplayLink USB-C Dual-4K Dock (GenesysLogic GL352x hub, FW 0x9305), 2x HP E273 (1080p @ 60Hz)**

## The Key Setting: `ForceScreenRefresh`

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

## All Hidden `defaults write` Keys

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

## Support Tool Discoveries

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

Config file location: `~/Library/Group Containers/73YQY62QM3.com.displaylink.DisplayLinkShared/Library/Application Support/Logs/.dlua.xml`

### `dlinfo` script

Collects a comprehensive system diagnostics zip including:
- Full system profile (SPX and text)
- IORegistry dumps (IOService and IOUSB)
- WindowServer preferences (system-wide and per-user)
- DisplayLink crash reports
- DisplayLink logs and EDID files
- DisplayLink process list

### `ResetDisplayPrefs.command` script

Nuclear option — deletes all display layout preferences:
- `/Library/Preferences/com.apple.windowserver.plist`
- Per-user `com.apple.windowserver.*.plist` in ByHost
- DisplayLink shared preferences
- Requires reboot after running

## XPC Service (Inter-Process Communication)

The `DisplayLinkXpcService` binary reveals DDC monitor control capabilities:

- **Brightness control** — set/get per-display brightness
- **Contrast control** — set/get per-display contrast
- **Flip setting** — per-display rotation/flip
- **Display enable/disable** — toggle individual displays

These are communicated between the user agent and a driver component via XPC.

## Firmware

### Bundled firmware packages

The DisplayLink Manager app includes 4 firmware packages for different dock chipsets:

| File | Codename | Type |
|---|---|---|
| `ella-dock-release.spkg` | EllaDockOW | Dock |
| `navarro-dock-release.spkg` | NavaDockOW | Dock |
| `ridge-dock-release.spkg` | — | Dock |
| `firefly-monitor-release.spkg` | — | Monitor |

All use the "ELLA" container format (likely Synaptics internal).

### USB Hub firmware (GenesysLogic GL352x)

The dock's USB hub uses a **GenesysLogic GL352x** chip (VID `0x05E3`, PID `0x0610`). A separate Windows-only firmware updater (`CADUAL4KDOCKPD_9392.exe` / "USB3.0 Hub Update Tool v3.001") can update the hub firmware from version 9311 to 9392, fixing random disconnection issues. This tool uses `GL_SS_HUB_ISP.exe` internally and flashes two ROM files (`Hub_1_9392_L1.ROM`, `Hub_1_9392_L2.ROM`).

Current hub firmware can be read from `bcdDevice` in IORegistry (e.g., decimal `37637` = `0x9305`).

## Internal Architecture

From binary analysis, the rendering pipeline consists of:

- **Screen capture** → ScreenCaptureKit (`SCStream`, `SCContentFilter`, `SCShareableContent`)
- **GPU diffing** → Metal shaders (`GpuDiffer`, `ShaderLibrary.metallib`) to detect changed regions
- **Zero-copy encoding** → `ZeroCopyGpuEncodeSurfacesWrapper` for efficient frame encoding
- **Healing frames** → `HealingFramesWatchdog` periodically sends full frames to fix corruption
- **Bandwidth allocation** → `BandwidthAllocator` manages throughput across multiple displays
- **Rendering** → `DLRenderPipeline` / `DLRenderEncoder` using Metal render pipeline
- **NivoViewer** → Per-display viewer with `ContentHint` (values: at least `Auto`), rotation, border adjustment

### Log & config locations

| Path | Description |
|---|---|
| `~/Library/Group Containers/73YQY62QM3.com.displaylink.DisplayLinkShared/Library/Application Support/Logs/` | Logs, EDID files, config XML |
| `*.log` | Application logs (encrypted/obfuscated) |
| `.dlua.xml` | User agent config (logging level, display configurations) |
| `*.edid` | Raw EDID data per monitor (256 bytes each) |
| `dlm_message_server_in#1` / `dlm_message_server_out#1` | Named pipes for IPC |

## How These Were Found

Keys and architecture details were extracted from the binaries using `strings` and correlated with `NSUserDefaults` access patterns (`boolForKey:`, `stringForKey:`, etc.). The `ForceBackendAPI` setting was verified using `sample` (call stack sampling) — both default and forced modes showed `SCStream` / `SCShareableContent` calls, with zero `CGDisplayStream` calls, confirming v15.1.0 already uses ScreenCaptureKit natively.

Support Tool scripts (`dlinfo`, `dlclearlogs`, `ResetDisplayPrefs.command`) were read directly as they are plaintext shell scripts.

For deeper analysis (e.g., how `ForceScreenRefresh` modifies the capture loop, valid `ContentHint` values beyond `Auto`, `BandwidthAllocator` tuning), Ghidra disassembly of the stripped Mach-O binary would be needed.

## Notes

- These are **undocumented internal preferences** — they may change or disappear with any DisplayLink update.
- DisplayLink on macOS works via screen capture (not native GPU output), which is a fundamental architectural limitation imposed by Apple. These tweaks help but can't make it match a native display connection.
- Tested April 2026. YMMV on different versions.
