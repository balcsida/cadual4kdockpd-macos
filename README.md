# i-tec CADUAL4KDOCKPD tools

Notes, scripts and tools for the i-tec CADUAL4KDOCKPD dock (USB-C, DisplayLink, 85 W power delivery):
the hub firmware update, the plug-in loop investigation and the fix for it. The investigation was done
on Macs; the tool that applies the fix runs on macOS and on Windows. Not affiliated with i-tec, Good
Way, Texas Instruments, Genesys Logic or Cable Matters.

## The plug-in loop, in short

- On an Apple silicon MacBook Pro the dock connected on about 13 of 39 fresh plug-ins. On the others it
  attached and detached about every 0.85 s without end, until the cable was replugged.
- The outcome follows who becomes the power source first: when the dock is first, it works; when the Mac
  is first, it loops.
- Updating the dock's two USB hubs from firmware 93.05 to 93.92 did not change this.
- The dock's TI TPS65982 power controller is configured with Try.SRC on. With that one bit cleared
  (written over USB through the dock's own management chip), 11 of 11 plug-ins connected on the tested
  unit, including 7 that started with the Mac as the power source.
- Evidence so far: one dock, one Mac, one cable, one afternoon. Full account: `dock-capture/REPORT.md`.

## What is here

| Path | Content |
|---|---|
| `cadual4kdockpd-firmware-update-macos-fwupdtool.md`, `update-cadual4kdockpd-firmware-macos.sh`, `test-update-cadual4kdockpd-firmware-macos.sh` | Updating the dock's Genesys hubs to 93.92 from macOS with `fwupdtool` |
| `displaylink-macos-performance-tuning.md` | DisplayLink settings and architecture notes |
| `dock-capture/REPORT.md` | The plug-in loop investigation: captures, tests, cause, the change and its result |
| `dock-capture/sampler.py`, `mark.sh`, `replugs.py` and the other scripts next to them | Mac-side capture (USB, power and port-controller state at 4 Hz) and its analysis |
| `dock-capture/gw-updater/PROTOCOL.md` | USB HID protocol of the dock's management chip (Good Way, `065f:f60f`), how the power controller's flash was read and written through it, and the run sheet |
| `dock-capture/gw-updater/GHIDRA.md` | How to load the firmware files into Ghidra |
| `dock-capture/gw-updater/tools/` | Read-only query tools, the TPS65982 image checker and patcher, the flash write tool and its offline rehearsal |
| `cadual4kdockpd-patch/` | App and command-line tool that applies the Try.SRC change to a dock of this exact model and, on macOS, can install the hub firmware: a Rust engine with a SwiftUI window for macOS and a Win32 window for Windows. Read its README first |

## Downloads

The [Releases](../../releases) page holds the app that applies the fix, built by GitHub from this
repository and without any firmware in it: `CADUAL4KDOCKPD-Patch-macos.zip` (the Mac app, Apple silicon
and Intel) and `CADUAL4KDOCKPD-Patch-App.exe` (Windows, nothing to install). The Mac app is signed for
local use only, so run `xattr -dr com.apple.quarantine "CADUAL4KDOCKPD Patch.app"` after unzipping it.
Read `cadual4kdockpd-patch/README.md` before using either.

## Before you write anything to a dock

The management chip can only write the power controller's flash after erasing the whole chip. If the
dock loses its own power between the erase and a verified write, it will not start again without an
external flash programmer. The write tool refuses a dock whose flash content differs from the one
described here. Read `dock-capture/gw-updater/PROTOCOL.md` first. You do this at your own risk.

## Not in this repository

Firmware images, flash dumps, compiled tools, captures and logs are left out: they are vendor binaries
or contain device serial numbers. The notes refer to them by file name (for example
`itec-ti-flash-original.bin`); `PROTOCOL.md` describes how each one was obtained.
