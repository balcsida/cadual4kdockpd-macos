# DisplayLink Hidden Settings & Architecture (macOS + Linux + Windows)

Undocumented preferences and internal architecture of DisplayLink drivers, reverse-engineered from the macOS binary (v15.1.0, re-checked against v17.1.56), the Linux driver (v6.2.0-30) and the Windows driver (v12.2.1977.0). The macOS claims are checked against Ghidra disassembly.

Originally tested on: **Apple M3 Max, macOS Tahoe 26.4, DisplayLink Manager 15.1.0, DisplayLink USB-C Dual-4K Dock (GenesysLogic GL352x hub, FW 0x9305), 2x HP E273 (1080p @ 60Hz)**

Re-checked in November 2026 against **DisplayLink Manager 17.1.56** (the current release, 5 November 2026) on macOS 27.2.1. That pass was static only: the arm64 slice was disassembled and compared with the 15.1.0 binary. The dock was not attached, so nothing in it was measured on a running display.

## What the 17.1.56 re-check changed

- `ForceScreenRefresh` is not a preference. Neither 15.1.0 nor 17.1.56 reads it from `NSUserDefaults`. The string is the name of an internal `NSNotification`, so `defaults write ... ForceScreenRefresh` does nothing. The earlier version of this document got this wrong.
- The preference the frame-processing function really reads is `DisableGpuDiffing`. 17.1.56 adds `UseGpuDiffing`.
- The GPU differ is only created for DL-6xxx devices driving a mode of 3840x2160 or more. On the 1080p monitors used here it never exists, so there is nothing to turn off.
- Without a differ the driver sends the dirty rectangles ScreenCaptureKit reports, not the whole screen.
- 17.1.56 no longer clamps the capture rate to 60 or 120 Hz.
- `DisableRestartOnCrash` is gone in 17.1.56. `EnableExperimentalModes` is in neither binary.
- New keys in 17.1.56: `UseGpuDiffing`, `VisualiseDirtyRects`, `DisableAccessibilityFilterSupport`, `DisableBrightnessContrast`, `DisableUpdater`, `AutoUpdateDebugCheckInterval`.
- `Scramble` and `FirmwareTrace` are in the macOS binary after all. They are built inline in the code, where `strings` does not see them. `Scramble` is a transport setting, not HDCP, and `FirmwareTrace` is a log name, not a setting.
- `WindowSize` is read in one place only, under `Nivolib/Network`, with a default of 4.

---

## macOS: GPU diffing (`DisableGpuDiffing`, `UseGpuDiffing`) and why `ForceScreenRefresh` does nothing

### `ForceScreenRefresh` is a notification name

`ForceScreenRefresh` appears once in the binary, as a `CFString` constant. Every reference to it is an `NSNotificationCenter` call:

- three functions post it with the display ID as an `NSNumber` (`postNotificationName:object:`), one of them right after `SourceModeChangeCompleted`
- one function observes it (`addObserverForName:object:queue:usingBlock:`). It sets up a per-display frames watchdog during `BaseScreenRecorder` init, with a dispatch queue labelled `StreamFramesWatchdog ...`. The watchdog calls back into the recorder, presumably to queue a full-screen refresh (the callback was not traced)

No `boolForKey:` or `objectForKey:` call takes it as an argument, in 15.1.0 or in 17.1.56. So this does nothing:

```bash
defaults write com.displaylink.DisplayLinkUserAgent ForceScreenRefresh -bool true
```

It is harmless if you set it earlier. To remove it:

```bash
defaults delete com.displaylink.DisplayLinkUserAgent ForceScreenRefresh
```

### What the frame-processing function reads

`BaseScreenRecorder` has a method (`FUN_1000efb60` in 17.1.56, `FUN_100050310` in 15.1.0) that turns a captured `IOSurface` into the list of rectangles to send. The render loop calls it for every frame with the previous surface, the new surface and the dirty rectangles the capture backend reported. In 17.1.56:

1. If `gpuDifferCreationRequired` is set and there is no `GpuDiffer` yet: read `DisableGpuDiffing`. If it is false (the default), create a `GpuDiffer` for the surface size. Clear `gpuDifferCreationRequired` either way.
2. If `gpuDiffingOverrideEnabled` is set (new in 17.x): read `UseGpuDiffing`. False drops the differ. True creates one if there is none.
3. If a differ exists and returns rectangles, use them.
4. Otherwise use the rectangles the backend passed in. If it passed none, use one rectangle covering the whole surface.

`gpuDiffingOverrideEnabled` is set once, when the recorder is created, to whether the `UseGpuDiffing` key exists at all (`objectForKey:` is not nil). 15.1.0 has steps 1, 3 and 4 only.

### When a differ is created at all

`gpuDifferCreationRequired` is not a constant. The C++ side fills it into the recorder config (`FUN_1000b40ec` in 17.1.56, `FUN_100096280` in 15.1.0):

```
gpuDifferCreationRequired = (device family field == 0x6000) && (width * height >= 3840 * 2160)
```

`0x6000` is presumably the DL-6xxx family (the Dual-4K dock's DL-6950 is one). The logic is the same in both versions.

| Setup | Differ by default | `DisableGpuDiffing=true` | `UseGpuDiffing=true` (17.x) | `UseGpuDiffing=false` (17.x) |
|---|---|---|---|---|
| DL-6xxx, mode of 3840x2160 or more | yes | no differ | differ | no differ |
| Anything else (for example 1080p or 1440p on this dock) | no | no change | differ | no change |

With no differ, the rectangles sent are ScreenCaptureKit's own dirty rectangles (`SCStreamFrameInfoDirtyRects`). The earlier claim that every frame becomes a full-screen update was wrong: a full rectangle is only used when the backend reports none.

### What this means for the April 2026 result

The "noticeably smoother" result recorded below for `ForceScreenRefresh=true` cannot have come from the key. On 2x 1080p there was no differ to disable either. The only thing each test really did was restart DisplayLink Manager.

For a 4K monitor on a DL-6xxx dock the real switches are:

```bash
# 15.x and 17.x: do not create the GPU differ
defaults write com.displaylink.DisplayLinkUserAgent DisableGpuDiffing -bool true

# 17.x: force it off (false) or on (true), at any resolution
defaults write com.displaylink.DisplayLinkUserAgent UseGpuDiffing -bool false
```

Restart DisplayLink afterwards, so the recorders are rebuilt:

```bash
killall DisplayLinkUserAgent && sleep 2 && open "/Applications/DisplayLink Manager.app"
```

Neither key has been tested on hardware, so whether the picture is better with or without the differ is open. 17.1.56 also has `VisualiseDirtyRects`, which creates a `DirtyRectVisualiser` for the recorder and should show what is being sent.

---

## macOS: Screen Recorder Architecture (Ghidra-verified)

DisplayLink implements **three screen capture backends** plus a runtime switcher. All four classes are still in 17.1.56:

| Class | Backend API | Role |
|---|---|---|
| `SCKitScreenRecorder` | ScreenCaptureKit (`SCStream`, `SCContentFilter`) | Default on macOS 15 and later |
| `CGDSScreenRecorder` | `CGDisplayStreamCreateWithDispatchQueue` | Used on macOS 14, and as a fallback |
| `AVFoundationScreenRecorder` | `AVCaptureVideoDataOutput` | Last fallback |
| `SwitchingScreenRecorder` | Wraps primary + fallback | Only built when `DetectVideoOverlay` is on |

`SwitchingScreenRecorder::init` takes a **primary** and **fallback** recorder. It maintains a dispatch queue, tracks the current active backend, and stores the last gamma ramp event and display data for seamless switching.

### Backend selection (17.1.56, `FUN_1000c7260`)

1. `ForceBackendAPI` (string): `ScreenCaptureKit`, `CGDS` or `AVFoundation` picks that backend. Any other value is ignored.
2. New in 17.x: if an internal flag on the display source is false, use `CGDSScreenRecorder` when it is available. The flag starts as true and one event handler (`FUN_1000b8cf8`) clears it, so this looks like a fallback for when ScreenCaptureKit cannot be used. What triggers it was not worked out.
3. Before macOS 15 (17.x still runs on macOS 14): `CGDSScreenRecorder`.
4. `DetectVideoOverlay` off (the default): `SCKitScreenRecorder`.
5. `DetectVideoOverlay` on: `SwitchingScreenRecorder` with `SCKitScreenRecorder` as primary. The fallback is the backend named by `FallbackBackendAPI`, else `CGDSScreenRecorder`, else `AVFoundationScreenRecorder`.

"Available" for CGDS means `dlsym` finds `CGDisplayCreateImage`, `CGDisplayStreamCreateWithDispatchQueue`, `CGDisplayStreamStart`, `CGDisplayStreamStop` and `CGDisplayStreamUpdateGetRects` in CoreGraphics. All five still resolve on macOS 27.2.1.

15.1.0 (`FUN_1000a5b54`) has the same order without step 2.

### Frame Rate Configuration (Ghidra-verified)

`SCStreamConfiguration.minimumFrameInterval` is set in `FUN_1000e9e3c` (17.1.56):

1. `FUN_1000e9bac` walks `NSScreen.screens`, finds the screen whose display ID matches the recorder's and returns its `maximumFramesPerSecond` (0 if none matches)
2. If that equals the refresh rate in the recorder config: `kCMTimeZero`, so no minimum interval
3. Otherwise: `CMTime(1, refreshRate)`

15.1.0 (`FUN_10004b5b4`) clamped first: a refresh rate of 120 or more gave `CMTime(1, 120)`, 61 to 119 gave `CMTime(1, 60)`, and only 60 or less went through steps 2 and 3. 17.1.56 has no clamp.

The other `SCStreamConfiguration` properties are fixed and unchanged from 15.1.0:
- `pixelFormat`: `0x42475241` (BGRA)
- `queueDepth`: 7
- `showsCursor`: true
- `width` / `height`: the stream size
- `scalesToFit`: false
- `capturesAudio`: false
- `excludesCurrentProcessAudio`: true

---

## macOS: All Hidden `defaults write` Keys

`NSUserDefaults` keys the `DisplayLinkUserAgent` binary (17.1.56) reads, in the `com.displaylink.DisplayLinkUserAgent` domain. The list comes from the call sites of `boolForKey:`, `stringForKey:`, `integerForKey:` and `objectForKey:`, not from `strings`: Swift keeps strings of 15 bytes or fewer inline in the code, so `UseGpuDiffing`, `DisableUpdater` and `ShowMenuBarIcon` never show up in a string dump.

### Performance & Rendering

| Key | Type | Verified | Description |
|---|---|---|---|
| `DisableGpuDiffing` | Bool | Ghidra ✓ | Do not create the Metal `GpuDiffer`. Only matters where one would be created: DL-6xxx at 3840x2160 or more (see above) |
| `UseGpuDiffing` | Bool | Ghidra ✓ (17.x) | When the key exists: true forces a `GpuDiffer` at any resolution, false removes it. Wins over `DisableGpuDiffing` |
| `VisualiseDirtyRects` | Bool | Ghidra ✓ (17.x) | Read when a recorder is created. Creates a `DirtyRectVisualiser`. Untested |
| `ForceBackendAPI` | String | Ghidra ✓ | `ScreenCaptureKit`, `CGDS` or `AVFoundation`. KVO-observed |
| `FallbackBackendAPI` | String | Ghidra ✓ | Same values. Only read when `DetectVideoOverlay` is on |
| `DetectVideoOverlay` | Bool | Ghidra ✓ | Auto-detect video call overlays. Has a `UserSettings` setter, so it is a Settings-tab switch. Wraps capture in `SwitchingScreenRecorder`. **Known performance issue** when enabled (CPU spikes and lag, see [BetterDisplay #4905](https://github.com/waydabber/BetterDisplay/discussions/4905)) |
| `SilentRestartsThreshold` | Integer | Ghidra ✓ | Read in `CaptureStreamOutput` init. 6 when unset or 0 |
| `ForceScreenRefresh` | n/a | Ghidra ✓ | **Not a preference.** An `NSNotification` name, never read from defaults |

### Display Modes

| Key | Type | Verified | Description |
|---|---|---|---|
| `EnableLargeHiDPIModes` | Bool | Ghidra ✓ | The "Additional HiDPI modes" switch in the Settings tab: extra 3008x1692 and 2560x1440 HiDPI modes. [On by default since 15.0](https://support.displaylink.com/knowledgebase/articles/1993915-displaylink-manager-app-experimental-hidpi-modes) |
| `EnableExperimentalModes` | n/a | not found | The string is in neither 15.1.0 nor 17.1.56, and the knowledge base article no longer gives a `defaults` command. Setting it does nothing |

### Color Processing

| Key | Type | Verified | Description |
|---|---|---|---|
| `DisableFluxGammaSupport` | Bool | Ghidra ✓ | Skips f.lux color processing integration. Read in two places |
| `DisableNightShiftSupport` | Bool | Ghidra ✓ | When true, skips `dlopen` of `CoreBrightness.framework` entirely, preventing Night Shift hooks. The `NightShiftSingleton` is never initialized |
| `DisableAccessibilityFilterSupport` | Bool | Ghidra ✓ (17.x) | Clears the accessibility-filter flag in the recorder config. Presumably stops the macOS colour filters being applied to DisplayLink displays |
| `DisableBrightnessContrast` | Bool | Ghidra ✓ (17.x) | Read next to the `brightness_and_contrast_monitor` string. Presumably turns off the brightness and contrast controls |

### Application Behavior

| Key | Type | Verified | Description |
|---|---|---|---|
| `ShowMenuBarIcon` | Bool | Ghidra ✓ | Hide/show the menu bar icon. Read in `UserSettings::init` |
| `SleepOnClamshell` | Bool | Ghidra ✓ | Has a `UserSettings` setter, so presumably a Settings-tab switch. Also in 15.1.0 |
| `AppAutostart` | Bool | Ghidra ✓ | 17.1.56 only writes it, from the autostart notification action. 15.1.0 also read it |
| `DisableUpdater` | Bool | Ghidra ✓ (17.x) | Read in the new update code. Presumably turns off the periodic update check added in 17.0 |
| `AutoUpdateDebugCheckInterval` | Bool | Ghidra ✓ (17.x) | Debug switch in the same code |
| `DisableRestartOnCrash` | n/a | removed | 15.1.0 read it before registering an `SMAppService` agent for restart on crash. 17.1.56 has neither the string nor the read. `RestartOnCrashService` still exists |

The app also keeps its own state in the same domain: `EnabledDisplays`, `DisplaysDisabledBySystem`, `LastAvailableUpdate`, `SUSkippedVersion`, `ScreenRecordingPermissionPreviouslyGranted`, `FirstSetupCompleted` and a few more.

### KVO-Observed Keys (Runtime Changes Without Restart)

`UserSettings::init` in 17.1.56 registers one `observeValueForKeyPath:` observer, for `ForceBackendAPI`. 15.1.0 registered two, `AppAutostart` and `ForceBackendAPI`.

`UseGpuDiffing` is read again on every frame, but only if the key existed when the recorder was created. Everything else is read once, so restart the app after changing it.

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
| `WindowSize` | `Nivolib/Network/WindowSize` (the only read in 17.1.56; `Core/WindowSize` is not read) | Window size of the Nivo transport, integer, default 4 | Larger = more throughput, more latency |
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
| `AdjustedDisplayMode/OriginalMode` | `0x0x0@0` | `<width>x<height>x<depth>@<freq>` |
| `AntiAutoAdjustingBorder/Colour` | `0` | Border color |
| `AntiAutoAdjustingBorder/Thickness_H` | `0` | Horizontal border thickness |
| `AntiAutoAdjustingBorder/Thickness_V` | `0` | Vertical border thickness |

### Logger configuration (can be added to `.dlua.xml`)

Based on the Linux `DLSupportTool.sh --debug` implementation, this XML structure enables debug logging on Linux. **On macOS it is ignored** (tested on 15.1.0 in April 2026, see Experiments). The log files are still encrypted in 17.1.56:

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

### Keys from the Linux driver, checked against the macOS binary

| Key | Description | In the macOS binary (17.1.56)? |
|---|---|---|
| `RenderRate` | Controls render rate in the update engine | No |
| `showFPS` | Displays FPS counter | No |
| `Powerups` | Power-on sequence control | No |
| `Scramble` | Scrambling of the Nivo transport (not HDCP) | Yes, a real setting (see below) |
| `FirmwareTrace` | Firmware-level trace logging | Yes, but only as a log name (see below) |

"No" means neither a string nor inline immediates for it in the arm64 slice. `Scramble` and `FirmwareTrace` were listed as Linux-only before, because `strings` cannot see inline strings.

### `Scramble` and `WindowSize` (Nivo transport)

`FUN_1002837cc` sets up the Nivo transport. Nivo is DisplayLink's device protocol (the monitors here show up as `NivoViewer` in `.dlua.xml`). The function opens a `Nivolib` > `Network` node in the settings hive and reads two values:

| Key | Type | Default | What it does |
|---|---|---|---|
| `WindowSize` | Integer | 4 | Transport window size. A line is logged when it differs from the current value |
| `Scramble` | Bool | true | Scramble every outgoing data block |

With `Scramble` on, the send path (`FUN_1002a2c48`) passes each outgoing block and the low byte of its sequence number to a scrambler object before queuing it. With it off the blocks go out as they are, and the driver logs a level 1 message at startup. Nothing here touches HDCP, so the earlier "HDCP scrambling control" description was a guess.

The parent path of the `Nivolib` node was not resolved, and nothing here was tested. Turning `Scramble` off is not a performance setting: if the device still expects scrambled blocks, the likely result is no picture.

### `FirmwareTrace` (log name)

On macOS `FirmwareTrace` is not a setting. The string is built in the constructor of the device object (`FUN_10018a604`) and handed to it as a name, and it is the only place the string occurs. It matches `FirmwareTrace.log` in the Logs folder: the dock's firmware trace output, encrypted like the other logs and rotated to `FirmwareTrace.old.log` at about 10 MB. 17.1.56 writes it without any setting being turned on. No switch for it was found.

The same constructor reads two settings that were not in this document before: `ForceHDMICompliance` (bool, default false) and `DualNivo` (bool, default true). Neither was looked into further.

---

## macOS: Support Tool Utilities

The **DisplayLink Support Tool** (`/Applications/DisplayLink Support Tool.app`) is a separate download, not part of DisplayLink Manager. It bundles several useful utilities. This section describes Support Tool 7.0.0 and was not re-checked for 17.1.56.

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

Sizes in DisplayLink Manager 17.1.56 for macOS: ella 608 KB, navarro 1.7 MB, ridge 879 KB, firefly 357 KB. All four still start with the `ELLA` magic.

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
       │    ├─ minimumFrameInterval: kCMTimeZero, or CMTime(1, refreshRate) if the screen's max FPS differs
       │    ├─ queueDepth: 7
       │    ├─ pixelFormat: BGRA
       │    └─ showsCursor: true
       └─ CaptureStreamOutput (receives IOSurface frames)
            ├─ reads SCStreamFrameInfoStatus and SCStreamFrameInfoDirtyRects from each frame
            ├─ yields (surface, dirtyRects) into frameStream
            └─ silentRestartsThreshold: NSUserDefaults integerForKey "SilentRestartsThreshold" (default: 6)
```

### Processing Layer

```
BaseScreenRecorder (processes captured frames)
  ├─ nextFrameNeedsFullScreenUpdate: bool (set to true in init)
  ├─ gpuDiffingOverrideEnabled: bool (17.x: the UseGpuDiffing key exists)
  ├─ gpuDifferCreationRequired: bool (from the recorder config: DL-6xxx and >= 3840x2160)
  ├─ differ: GpuDiffer? (Metal-based frame differ)
  ├─ framesWatchdog: monitors frame delivery, observes the ForceScreenRefresh notification
  ├─ healingFramesWatchdog: periodically forces full frames
  ├─ frameTransformer: applies transformations
  ├─ gammaApplier: color correction
  ├─ shadowSurfaces: double-buffered IOSurfaces
  └─ latestSurface: most recent captured IOSurface
```

The render loop (`FUN_1000f01a4` in 17.1.56) takes `(surface, dirtyRects)` pairs off the stream. For each one it calls the frame-processing method with `latestSurface`, the new surface and the rectangles, then sends the surface with the rectangles that come back.

### Frame Processing Flow (17.1.56)

```
New (surface, dirtyRects) from the backend
  │
  ├─ gpuDifferCreationRequired?   (DL-6xxx and >= 3840x2160 only)
  │     └─ no differ yet and DisableGpuDiffing=false ──▶ create GpuDiffer (FUN_1000e3930)
  │
  ├─ gpuDiffingOverrideEnabled?   (17.x: the UseGpuDiffing key exists)
  │     ├─ UseGpuDiffing=false ──▶ drop the differ
  │     └─ UseGpuDiffing=true  ──▶ create GpuDiffer if missing
  │
  ├─ differ exists and returns rects ──▶ send those
  ├─ backend passed dirtyRects       ──▶ send those
  └─ otherwise                       ──▶ send one full-surface rect
```

### Log & Config Locations (macOS)

| Path | Description |
|---|---|
| `~/Library/Group Containers/73YQY62QM3.com.displaylink.DisplayLinkShared/Library/Application Support/Logs/` | Logs, EDID files, config XML |
| `DisplayLinkUserAgent.log`, `DisplayLink Manager.log`, `FirmwareTrace.log` (each with an `.old.log`) | Application and firmware logs (encrypted/obfuscated, also in 17.1.56) |
| `.dlua.xml` | User agent config (logging level, display configurations) |
| `*.edid` | Raw EDID data per monitor (256 bytes each) |
| `dlm_message_server_in#1` / `dlm_message_server_out#1` | Named pipes for IPC |

---

## Experiments Completed (April 2026, 15.1.0)

| Experiment | Result | Details |
|---|---|---|
| `ForceScreenRefresh=true` | **Not a valid test** | Felt noticeably smoother at the time. The key is never read (see above), so the change came from somewhere else, most likely the restart. |
| `ForceBackendAPI=ScreenCaptureKit` | **No effect** | v15.1.0 already uses ScreenCaptureKit natively. Stack trace sampling confirmed both modes use `SCStream` with zero `CGDisplayStream` calls. |
| Debug logging via `.dlua.xml` Logger XML | **Failed** | macOS binary ignores Logger XML config entirely. Logs remain encrypted. The Linux `DLSupportTool.sh --debug` XML structure is not read by the macOS driver. |
| `ContentHint=Graphics` in `.dlua.xml` | **No perceptible effect** | XML was accepted and survived restart, but no visible difference. |
| `DisableFluxGammaSupport=true` | **Applied** | Removes f.lux hook overhead. Minor/unmeasurable impact. |
| `DisableNightShiftSupport=true` | **Applied** | Skips CoreBrightness.framework loading. Minor/unmeasurable impact. |
| XML tunables | **Made things worse** | Tested `WindowSize=65536`, `BandwidthAllocator/WindowSize=65536`, `ContentHint=Graphics`, `ExtraModes=1`, `MaskProtocolErrors=1` all together. Result: "brutal latency." `ForceScreenRefresh` was off for this run, which makes no difference, so the tunables themselves caused it. Which one is unclear: 17.1.56 only reads `WindowSize` under `Nivolib/Network`. |

Nothing has been tested on hardware with 17.1.56 yet.

## Conclusion

There is no hidden preference that makes DisplayLink smoother on this setup (DL-6950 dock, 2x 1080p). `ForceScreenRefresh` is not read, and the GPU differ that `DisableGpuDiffing` controls is only created at 3840x2160 and above.

For a 4K display on a DL-6xxx dock, `DisableGpuDiffing` (15.x and 17.x) and `UseGpuDiffing` (17.x) do change the pipeline: they switch between the Metal differ and ScreenCaptureKit's dirty rectangles. Whether that looks better is untested.

The XML-based tunables (`WindowSize`, `BandwidthAllocator`, `ContentHint`, `ExtraModes`, `MaskProtocolErrors`) in `.dlua.xml` were tested in April 2026 and made latency worse. **Do not use them.**

---

## DisplayLink Official GitHub Repos

Analysis of [github.com/DisplayLink](https://github.com/DisplayLink) repositories:

### EVDI — Frame Pacing Bug ([#512](https://github.com/DisplayLink/evdi/issues/512), [#562](https://github.com/DisplayLink/evdi/pull/562))

[Issue #512](https://github.com/DisplayLink/evdi/issues/512) describes how the EVDI vblank timing is fundamentally broken:

> *"evdi fires vblank immediately after the user-space driver called `DRM_EVDI_GRABPIX`... This in turn will generally result in the drm master immediately starting to render a new frame, thinking the display is fast enough to want it, which in turn leads to new update ready events... The result is wasted CPU/GPU on useless frames that get thrown away anyway."*

[PR #562](https://github.com/DisplayLink/evdi/pull/562) proposes a fix on Linux: **proper hrtimer-based vblank** at the display's actual refresh rate, the same pattern used by the `vkms` virtual display driver. As of November 2026 the issue is still open and the PR is not merged (latest EVDI release: v1.15.1, September 2026).

This is a Linux problem. The earlier version of this document tied it to `ForceScreenRefresh` and the `GpuDiffer` on macOS. Nothing in the macOS binary supports that link.

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

- [#552](https://github.com/DisplayLink/evdi/issues/552) — Low refresh rate (below 20 FPS visually despite 60 Hz reported) after EVDI update. Closed in February 2026.
- [#522](https://github.com/DisplayLink/evdi/issues/522) — DisplayLinkManager consuming 14% CPU with no monitors connected. Related to the update loop spinning without proper throttling. Still open.
- [#548](https://github.com/DisplayLink/evdi/pull/548) — CPU cache flush fix for cache coherency. Closed without being merged in February 2026.

---

## Methodology

- **macOS `defaults write` keys**: every call site of `boolForKey:`, `stringForKey:`, `integerForKey:`, `objectForKey:`, `dictionaryForKey:`, `arrayForKey:` and `setBool:forKey:` in the arm64 slice, with the key argument resolved from the code before the call. That covers `CFString` constants, Swift literals (`adrp`/`add`, then `sub #0x20`) and Swift small strings built from immediates. String dumps alone miss the last kind
- **15.1.0 vs 17.1.56**: the same scripts run on both binaries (Ghidra headless), so each claim above is either the same in both or marked with the version
- **ScreenCaptureKit verification** (15.1.0 only): `sample` (call stack sampling) confirmed both default and forced modes use `SCStream`, zero `CGDisplayStream` calls
- **Ghidra disassembly** of `DisplayLinkUserAgent` 17.1.56 (arm64 slice, 6.8 MB, stripped, image base `0x100000000`):
  - `FUN_10003f5bc`: `UserSettings::init`, reads the Settings-tab keys and registers the `ForceBackendAPI` observer
  - `FUN_1000efb60`: frame processing method, reads `DisableGpuDiffing` and `UseGpuDiffing`, creates the `GpuDiffer` through `FUN_1000e3930`
  - `FUN_1000edc10`: `BaseScreenRecorder` init body, reads `VisualiseDirtyRects`, sets `gpuDiffingOverrideEnabled` and `gpuDifferCreationRequired`
  - `FUN_1000f01a4`: render loop that calls the frame processing method
  - `FUN_1000b40ec`: builds the recorder config, including `gpuDifferCreationRequired` (the 0x6000 and 3840x2160 test at `0x1000b4b04`)
  - `FUN_1000c7260`: recorder factory (`ForceBackendAPI`, `FallbackBackendAPI`, `DetectVideoOverlay`). `FUN_1000c71a4` maps the backend name to a class, `FUN_1000dbaf0` probes for the CGDisplayStream symbols
  - `FUN_1000e9e3c`: `SCStreamConfiguration` setup. `FUN_1000e9bac`: the `NSScreen.maximumFramesPerSecond` lookup
  - `FUN_1000ea50c`: `CaptureStreamOutput` frame handler (dirty rects). `FUN_1000e8728`: its init, reads `SilentRestartsThreshold`
  - `FUN_1000cdbbc`: frames watchdog, observes `ForceScreenRefresh`. `FUN_1000b72a8`, `FUN_1000c3f20`, `FUN_1000c951c`: the three places that post it
  - `FUN_1002837cc`: Nivo transport setup, reads `WindowSize` and `Scramble`. `FUN_1002a2c48`: the send path that scrambles. `FUN_10018a604`: device object constructor (`FirmwareTrace`, `ForceHDMICompliance`, `DualNivo`)
  - `FUN_1000e3a9c`: reads `DisableNightShiftSupport`. `FUN_1000ba8d8`: reads `DisableAccessibilityFilterSupport`. `FUN_1000cc33c`: reads `DisableBrightnessContrast`
- **Same functions in 15.1.0** (arm64, 6.5 MB): `UserSettings::init` `0x10002a410`, frame processing `FUN_100050310` (the key string at `0x100413c60` is `DisableGpuDiffing`), recorder config `FUN_100096280`, recorder factory `FUN_1000a5b54`, frame interval `FUN_10004b5b4`, `SCStreamConfiguration` setup `FUN_10004b66c`, `ForceScreenRefresh` observer `FUN_1000ac5b4`, `NightShiftSingleton` init `FUN_1000480f0`, `AppDelegate` init `FUN_10000adec` (reads `DisableRestartOnCrash`)
- **Linux driver**: Extracted from Makeself archive (`tail -n +714 | gzip -cd | tar xf -`)
- **EVDI parameters**: From kernel module source code (`evdi_params.c`, `evdi_sysfs.c`)
- **EVDI library API**: From `evdi_lib.h` — `evdi_connect2()` bandwidth parameters
- **EVDI GitHub issues**: Frame pacing bug ([#512](https://github.com/DisplayLink/evdi/issues/512)), vblank fix ([#562](https://github.com/DisplayLink/evdi/pull/562)), Asahi Linux support ([#559](https://github.com/DisplayLink/evdi/pull/559))
- **Cross-platform XML keys**: Found by cross-referencing strings in Linux `DisplayLinkManager` (aarch64) and macOS `DisplayLinkUserAgent` (arm64). All of them are still in 17.1.56
- **ContentHint values**: From Linux `DisplayLinkManager` binary strings. No `Game`/`Cinema`/`Photo`/`Graphics` hint values were found in the macOS binary (strings or inline immediates), only EDID content-type labels
- **Windows driver**: Extracted from PE32 installer → 7z SFX → driver packages. Registry settings from embedded `ImportSettings.reg` and `dlcdcncm.inf`
- **DL-7450 SDK**: From [github.com/DisplayLink/dl-7450](https://github.com/DisplayLink/dl-7450) — MicroPython dock programming interface

## Notes

- Hidden preferences may change or disappear with any DisplayLink update. `DisableRestartOnCrash` went between 15.1.0 and 17.1.56
- DisplayLink on macOS works via screen capture (not native GPU output) — a fundamental Apple limitation
- The Linux driver uses EVDI (virtual display interface in kernel) + DRM, which is architecturally different from macOS
- Windows uses native GPU driver integration (WDDM), not screen capture — fundamentally different from both macOS and Linux
- The Linux and Windows sections describe 6.2.0-30 and 12.2.1977.0 and were not re-checked. EVDI upstream is at v1.15.1
- Tested on hardware in April 2026 (15.1.0). Re-checked by disassembly only in November 2026 (17.1.56). YMMV on different versions
