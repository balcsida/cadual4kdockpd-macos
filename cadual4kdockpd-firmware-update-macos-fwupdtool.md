# Updating CADUAL4KDOCKPD Hub Firmware on macOS with fwupdtool

This updates the Genesys Logic USB hub firmware inside the i-tec
`CADUAL4KDOCKPD` / USB-C Dual-4K Dock using `fwupdtool` on macOS.

The vendor patch package is Windows-oriented, but it contains raw Genesys hub
firmware images that `fwupdtool` can flash directly:

- `Hub_1_9392_L1.ROM`
- `Hub_1_9392_L2.ROM`

The update moves the hub firmware to `93.92` / `0x9392`, which is the vendor
patch for random dock disconnects.

## Warnings

Firmware flashing can brick hardware if interrupted. Keep the dock powered,
keep the Mac powered, and do not unplug USB-C while erase/write/verify is in
progress.

Before flashing, disconnect devices from the dock where practical. Leave only:

- dock power
- USB-C connection to the Mac

## Install Tools

```bash
brew install fwupd jq
```

No extra extraction tool is needed: `bsdtar`, which ships with macOS, reads both
the vendor RAR and the self-extracting updater inside it. `unrar` works too
(`brew install unrar`). Homebrew's `7zz`/`7z` fail on this RAR with
"Unsupported Method", so the updater script tries `bsdtar`, `7zz`, `7z` and
`unrar` in that order and uses the first one that succeeds. It skips extraction
when ROM files with the SHA-256 values below are already in the work dir.

Homebrew's `fwupd` package provides `fwupdtool`. On macOS this is enough; a
Linux-style `fwupd` daemon or `fwupdmgr` is not required for this workflow.

Confirm the Genesys plugin is available:

```bash
fwupdtool get-plugins | rg genesys
```

## Automated Script

This gist includes a walkthrough script for the full process:

```bash
./update-cadual4kdockpd-firmware-macos.sh
```

By default, the script is a dry run. It extracts and validates the firmware,
reports the detected hub versions, and prints the `fwupdtool install-blob`
commands it would run, but it does not write firmware.

Useful safe modes:

```bash
# Explicitly walk through detection and show flash commands without writing firmware
./update-cadual4kdockpd-firmware-macos.sh --dry-run

# Extract, inspect, and report current versions only
./update-cadual4kdockpd-firmware-macos.sh --no-flash
```

Firmware writes require an explicit `--flash`:

```bash
./update-cadual4kdockpd-firmware-macos.sh --flash
```

The script reports the detected hub firmware versions, maps `HUB_16` to
`Hub_1_9392_L1.ROM` and `HUB_00` to `Hub_1_9392_L2.ROM`, asks for confirmation
before firmware writes, and explains the known macOS final attach/reset warning
if the hub already reports the target version after verify.

## Extract the Firmware ROMs

Start with the vendor archive:

```bash
RAR=~/Downloads/Patch_CADUAL4KDOCKPD.rar
OUT=~/Downloads/Patch_CADUAL4KDOCKPD_firmware

mkdir -p "$OUT/outer" "$OUT/roms"
bsdtar -xf "$RAR" -C "$OUT/outer" Patch_CADUAL4KDOCKPD/CADUAL4KDOCKPD_9392.exe
bsdtar -xf "$OUT/outer/Patch_CADUAL4KDOCKPD/CADUAL4KDOCKPD_9392.exe" \
  -C "$OUT/roms" Hub_1_9392_L1.ROM Hub_1_9392_L2.ROM
```

Expected files:

```bash
ls -lh "$OUT/roms"
shasum -a 256 "$OUT/roms"/Hub_1_9392_L*.ROM
```

Known SHA256 values from the i-tec package:

```text
9dc14b65f251de4a74aa8f5a613d90607a87333ff71fc26079eb82d4e7c69f6c  Hub_1_9392_L1.ROM
69ba8fe38a9c020bcca428bb795e10e177b2e5101fa58cfa2270c61df1ca3880  Hub_1_9392_L2.ROM
```

If you use `unrar` instead and it is killed by macOS quarantine, remove
quarantine from the Homebrew
binary and try again:

```bash
sudo xattr -rc "$(command -v unrar)" "$(realpath "$(command -v unrar)")"
```

## Check the ROM Metadata

Both ROMs should parse as Genesys USB hub firmware version `93.92`:

```bash
fwupdtool --plugins genesys firmware-parse "$OUT/roms/Hub_1_9392_L1.ROM" genesys-usbhub
fwupdtool --plugins genesys firmware-parse "$OUT/roms/Hub_1_9392_L2.ROM" genesys-usbhub
```

Expected details include:

```text
version 93.92
version_raw 0x9392
mask_project_ic_type 352310
running_project_ic_type 352310
```

## Identify the Dock Hub Devices

List the updateable Genesys hub devices:

```bash
fwupdtool --plugins genesys get-devices --show-all --json \
  | jq '.Devices[] | {Name, DeviceId, InstanceIds, Version, VersionRaw}'
```

On this dock there are two updateable USB2 hub entries, usually:

- `USB\VID_05E3&PID_0610&HUB_16`
- `USB\VID_05E3&PID_0610&HUB_00`

Before the update they may report `Version` `93.5` and `VersionRaw` `37637`
(`0x9305`). After the update they should report `93.92` and `37778`
(`0x9392`).

In the tested dock, the mapping was:

| Firmware | Hub instance | Role |
|---|---|---|
| `Hub_1_9392_L1.ROM` | `HUB_16` | first/root hub layer |
| `Hub_1_9392_L2.ROM` | `HUB_00` | downstream hub layer |

## Flash the Firmware

Replace the two device IDs below with the IDs from your own
`fwupdtool get-devices` output.

```bash
L1_DEVICE_ID=672c087de09848d9e7ee32aa1dea2fbeb8b81e6b
L2_DEVICE_ID=de63d46dd16f065cc712a6f6f1d9698854b15053

fwupdtool --plugins genesys --prepare --cleanup -y install-blob \
  "$OUT/roms/Hub_1_9392_L1.ROM" \
  "$L1_DEVICE_ID" \
  93.92

fwupdtool --plugins genesys --prepare --cleanup -y install-blob \
  "$OUT/roms/Hub_1_9392_L2.ROM" \
  "$L2_DEVICE_ID" \
  93.92
```

The progress should pass through erase, write, and verify stages.

On macOS, `fwupdtool` may fail at the final attach/reset step with:

```text
failed to attach: failed to attach using genesys: error resetting device: USB error: Input/Output Error [-1]
```

If erase/write/verify completed first, this can still be a successful flash.
Verify the reported versions before retrying.

## Verify

Check `fwupdtool` first:

```bash
fwupdtool --plugins genesys get-devices --show-all --json \
  | jq '.Devices[] | {DeviceId, InstanceIds, Version, VersionRaw}'
```

Expected:

```text
Version: 93.92
VersionRaw: 37778
```

You can also verify through IORegistry:

```bash
ioreg -p IOUSB -l -w 0 | rg 'USB[23]\.1 Hub|bcdDevice|idVendor|idProduct'
```

Expected Genesys hub values:

```text
idVendor = 1507
idProduct = 1552    # USB2.1 hub, 0x0610
idProduct = 1568    # USB3.1 hub, 0x0620
bcdDevice = 37778   # 0x9392
```

## Power Cycle

After both hub layers report `93.92`, power-cycle the dock:

1. Unplug USB-C from the Mac.
2. Remove power from the dock.
3. Wait about 10 seconds.
4. Reconnect dock power.
5. Reconnect USB-C to the Mac.

After reconnecting, confirm both hub layers still report `93.92`.

## Notes

- This process updates the Genesys USB hub firmware, not the DisplayLink video
  firmware.
- `fwupdtool install-blob` is used because the vendor package provides raw ROM
  files rather than an LVFS `.cab`.
- Do not use random device IDs from examples. Always use the current IDs from
  your own `fwupdtool get-devices` output.
