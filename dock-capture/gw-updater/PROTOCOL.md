# Good Way HID protocol (USB 065f:f60f), from GW_IspTool E108 and its bundled EFM8 firmware

Source: static reading, then confirmed live on the i-tec dock on 2026-11-25 (commands 08, 07, 1B only). PC side in Ghidra (`DUD15X0D1_Update_E108.exe`), chip side with `d51.py`
(`python3 d51.py seg0-mcu-efm8-app.bin 3000 <start> <end>`). The i-tec dock's own dispatcher was read back with 1B and is identical to the Cable Matters one
(same jump table, same handler addresses), so this table applies to the i-tec dock.

## Frame, host to device (64 bytes, HID output report)

| Byte | Value | Note |
|---|---|---|
| 0 | `02` | report ID (PC: FUN_0041eea0 sets it) |
| 1 | `04` | constant in every command the tool builds |
| 2, 3 | `FE FF` | checked by the chip at 0x30BA-0x30D0 |
| 4 | command | dispatched at 0x30ED-0x3111, jump table at 0x3112 |
| 5..62 | arguments | zero-filled |
| 63 | `5A` | trailer, checked at 0x30D3-0x30DC. The bootloader commands 04/05 are sent without it. |

The chip keeps the frame at XDATA 0x00BB (`04`) .. 0x00F9 (`5A`); command at 0x00BE, first argument 0x00BF.
Replies are built at XDATA 0x0442.. and sent by 0x66A7 with the length in R5.

## Commands handled by the application firmware

| Cmd | Handler | Chip's own log text | What the code does | Class |
|---|---|---|---|---|
| 03 | 0x3136 | `JUMP ISP` | disables interrupts, calls 0x0000: enters the bootloader | DANGEROUS |
| 07 | 0x3168 | `USBUID_ISP_EFM8_CMD_DEVICE_STATUS` | replies `A0|flag, A5, x, x..`, 10 bytes | read only |
| 08 | 0x319C | `USBUID_ISP_EFM8_CMD_DEVICE_VERSION` | copies 12 bytes from code 0x020A-0x0215, replies 16 bytes | read only |
| 09 | 0x32C5 | `TI initial %d` | starts the TI update path | changes state |
| 0A | 0x3309 | `TI erase!` | erases TI flash | DESTRUCTIVE |
| 0B | 0x333C | `TI program!` | writes TI flash | DESTRUCTIVE |
| 0C | 0x32EC | `TI status!` | reads TI state through 0x6573, replies 8 bytes | probably read only, not verified |
| 0D | 0x334B | `TI checksum!` | 4 argument bytes (address/length), replies 6 bytes | probably read only, not verified |
| 0E | 0x33CD | `TI end!` | leaves the TI update path | changes state |
| 1B | 0x3266 | (none) | arg = index; copies 0x39 bytes from code 0x02AA + index*0x39, replies 63 bytes | read only |
| 1C | 0x3240 | `Wait reset device time -> %d` | stores arg as a reset delay, arms a reset | changes state |
| 04, 05, 06 and others | 0x33F3 | | ignored by the application; 04/05 are bootloader (ISP) commands | |

Commands the PC tool also builds for other targets (first command of each class): `1F`, `0F`, `14`. The
application firmware read here has no handler for them.

## Replies (device to host, 64 bytes): `02 | cmd FE FF cmd | data`

| Sent | Reply data | Meaning |
|---|---|---|
| `08` | `45 01 00 03 00..` | version 'E' 1 0 3 = **E103** |
| `07` | `A0 A5 00 00..` | status: application mode |
| `1B 00` | `00` + `E103        181203 000..` | table record 0: bundle E103, 2018-12-03 |
| `1B 01` | `01` + `...V1.38.6     181129 ` | TI firmware 1.38.6 (the TI image carries `0001.38.06`), 2018-11-29 |
| `1B 02..05` | zeros, record 5 ends `end` | no other components |
| `1B 06..FF` | raw code | code memory 0x0400-0x3BA9: saved as `itec-mcu-code-02AA-3BA9.bin` |

Command 1B reads code memory at 0x02AA + index*57, 57 bytes per reply, for any index 0-255.

## First live queries, in order, only after the owner agrees

1. `08` version. 2. `07` status. 3. `1B` with index 0, 1, 2.. (looks like the per-component version table;
compare with `seg8-text-e109.bin`).
Never send `03`, `09`-`0B`, `0E`, `1C`, `04`, `05`.

## Not known

- Whether a "read TI flash" command exists. None is in this table; `dump %d` at 0x6EB6 is a debug print
  whose caller has not been traced.
- `0D` takes a 32-bit length and returns a 16-bit checksum from the start of the TI flash (routine 0x4218,
  256-byte chunks). Whether it works outside a `09`..`0E` session is not known.

## TI flash read through the chip (2026-11-25, read only)

Session: `09` (enable the chip's SPI master on the TI flash, read flash ID; reply byte 5 = 0 on success),
`0C` (flash status registers; dock replied `00 02`), `0D` + 32-bit big-endian length (16-bit byte sum of
flash bytes 0..length-1, reply bytes 5-6, high byte first), `0E` (release the SPI pins). Routines in the
Cable Matters firmware: init 0x6DAF + 0x5296, status 0x6573, sum 0x4218 (reads 256-byte chunks with flash
command 0x0B through 0x67EE), release 0x6B41. The vendor tool uses the same `0D` as its verify step.
The bundle's version table stores these sums: TI image 0x7717, chip firmware 0x3EDD (both reproduced).

Read speed about 26 KB/s. A byte is read as S(n+1) - S(n). Files:
`itec-ti-sums-4k.csv`, `itec-ti-sums-256.csv`, `itec-ti-sums-bytes-2800.csv` (length, sum, seconds),
`itec-ti-flash-2800-29FF.bin` (exact bytes), `itec-ti-flash-reconstructed.bin` (see below).

Result: the dock's TI flash equals `seg5-ti-tps65982-image.bin` except 5 bytes in each header copy:

| Flash offset | Dock | Cable Matters | Meaning |
|---|---|---|---|
| 0x2804-0x2807 (and 0x16804) | `bc 9d d1 03` | `89 1c 26 4d` | checksum of the configuration block |
| 0x290F (and 0x1690F) | `71` | `70` | Control Configuration bit 24, AutomaticIDRequest: 1 on the dock |

Evidence level: exact for 0x2800-0x29FF; every other 4 KB block (and every 256-byte window of the two
header blocks) has the same sum as the Cable Matters image; flash is blank (FF) from 0x2A000 to 1 MB.
The reconstructed image reproduces all 615 measured sums.

Configuration block layout (flash 0x2800): `01 00`, length `13 04` (0x413), CRC, 8 zero bytes, `CST1`,
strings, then records `FF FF FF FF | register | 00 | n | n+1 data bytes`. Register meanings from TI
SLVUAN1A. On the dock:
- 0x28 System Configuration (data at 0x28F4): PortInfo 101b (source/sink, PR and DR swap),
  **TrySRCSupport = 1** (bit 2 of the byte at 0x28F9), Type-C current 3 A, all power switches source-only.
- 0x29 Control Configuration (data at 0x290C): ExternallyPowered, ProcessSwapToSink, ProcessSwapToSource,
  InitiateSwapToSource, ProcessSwapToUFP, InitiateSwapToUFP, AutomaticIDRequest.
- 0x32 source capabilities: 5 V, 9 V, 12 V, 15 V at 3 A and 20 V at 4.25 A.

Checksums (both reproduced on the Cable Matters image AND on the bytes read from the dock):
- application CRC at region+0x10 = bit-reversed(NOT zlib.crc32(application bytes));
- configuration CRC at block+4 = same function over block[0:4] + block[0xC:len], len = u16 at block+2.
  The dock's stored value matches with the dock's 512 exact bytes plus Cable Matters bytes for the rest,
  which proves the dock's whole configuration block equals the Cable Matters one except the one bit.
`tools/ti_image.py check <image>` validates pointers, both regions and all four CRCs.

Flash layout and boot (TI SLVUAH7B, fig. 3-4 and 3-5): low header at 0x0000 (pointer) / 0x0FFC (offset),
high header at 0x1000 / 0x1FFC; region = pointer + offset: low 0x2000, high 0x16000 on this dock.
The boot ROM reads the low region's configuration, then its application and CRC; if either is invalid it
uses the high region. It reads the flash only at boot.

## Write path (decoded; run on the dock on 2026-11-25, see the run sheet)

| Step | Frame | Chip routine | Effect |
|---|---|---|---|
| erase | `0A` | 0x3309 -> 0x6C01 | flash commands 06 then **C7: full chip erase**; sets write address 0, expected sequence 1, mode 2; replies at once |
| wait | `0C` | 0x6573 | flash status registers (reply bytes 5-6); the vendor tool polls until byte 5 is 0 |
| data | `02 n seq-lo seq-hi <59 bytes> 5A` | 0x3E21 (only in mode 2) | n <= 59, sequence from 1 (little-endian); wrong sequence or trailer: ignored without reply. Bytes go to a 256-byte buffer at XDATA 0x00FA; a frame with n < 59 programs the buffered bytes at the write address (0x5878, flash command 02, waits for completion) and advances it. Reply `02 3B seq-lo seq-hi` |
| verify | `0D` + length | 0x4218 | 16-bit byte sum, compared by the tool with the file's |
| end | `0E` | 0x33CD | mode 0, release SPI pins |

- The vendor tool sends every 256-byte page as 59+59+59+59+20 (PC routine FUN_0041a3e0).
- There is no way to program without the chip erase: mode 2 is set only by the erase handler.
- The buffer pointer and count (XDATA 0x0066, 0x0068) are reset only by a page write or a chip-firmware
  restart, not by `09`, `0A` or `0E`. A page left half sent would overflow into the write address on the
  next attempt. `tools/tiwrite.swift` therefore sends a 1-byte flush frame after the first erase and
  erases again.
- Command `0B` ("TI program!") exists in the chip but the vendor tool never sends it.
- The data handler lies above 0x3BA9, outside the part of the dock's own chip firmware that could be
  read. In the dock's firmware the call to it, its address (0x3E21), the erase handler and all text
  addresses up to 0x420D are identical to the Cable Matters build, so it is very probably the same code.

## Patched image (written to the dock on 2026-11-25 17:31, verified)

- `itec-ti-flash-original.bin`: the dock's present content, 0x29284 bytes, byte sum 7943.
- `itec-ti-flash-patched-trysrc0.bin`: the same with TrySRCSupport cleared in the LOW region only
  (0x28F9: 0C -> 08) and that region's configuration CRC updated (0x2804: BC 9D D1 03 -> 1E 27 AD C5).
  Five bytes differ. Byte sum 78C9. The high region is left original on purpose: if the boot ROM ever
  rejects the low region it falls back to the present behaviour.
- The new CRC was derived two ways that agree: the full recipe, and the one-bit delta rule that also
  maps the dock's CRC onto the Cable Matters one.
- `tools/tiwrite.swift`: `frames` (offline), `preflight` (read only), `write` (erase + program + verify,
  needs `--yes-erase-ti-flash`). Its frame stream was run through a model of the chip's handler
  (`tools/sim_chip_write.py`): the modelled flash equals the image, also when started with a half-filled
  buffer.

Risks of writing: (1) the dock losing its own power between the erase and the verified write (a few
minutes) leaves the TI flash empty, and recovery then needs an external flash programmer; (2) the patch
is an experiment: with Try.SRC off the dock stays sink when the Mac is source first, negotiates 5 V and
asks for a power-role swap (InitiateSwapToSource is set); whether that goes well on a Mac is not known;
(3) the dock's hubs may re-enumerate once during that swap. The new configuration loads only after the
dock is powered off and on. To go back, write `itec-ti-flash-original.bin` the same way.

## Checks done on 2026-11-25 afternoon (read only; nothing written to the dock)

Live, `tisession sums pending-trimmed-lengths.txt` (145 sums, `itec-ti-sums-pending.csv`):
- Exact bytes match `itec-ti-flash-original.bin` at 0x0FF8-0x1007, 0x1FF8-0x204F, 0x16000-0x16013 and
  0x16800-0x1680F (140 bytes, `tools/exact_bytes.py`). Both region headers store application CRC
  36ED13A6 and both configuration blocks store CRC 03D19DBC.
- S(2 MB) = S(1 MB) = 87C7: no wrap at 1 MB, so the flash is 2 MB or larger, and the second MB is empty.
  A full chip erase may take up to a minute. Not probed beyond 2 MB.
- Not read exactly: the high copy's registers 0x28/0x29 (implied by its CRC), and the application
  (implied by its CRC and the 4 KB sums).

Offline:
- TI SLVUAN1A table 3-6 re-read: TrySRCSupport is bit 10 of "bytes 5-8" of register 0x28, which is bit 2
  of flash byte 0x28F9. The other fields of the dock's value decode sensibly with the same layout.
- Chip routines re-read in the Cable Matters build: data handler 0x3E21-0x3FA9 (acknowledgement byte is
  the constant 3B), erase 0x6C01 (flash 06, C7), page program 0x5878 (flash 06, 02 + 3 address bytes;
  gives up after 20 status reads and the handler acknowledges anyway), end 0x33CD (flash 04, release pins,
  no reset of the TI controller). The buffer pointer is initialised to 0x00FA by the start-up table (0x62E7).
- Dock firmware E103 against Cable Matters E109, readable part 0x3000-0x3BA9: all 84 differing bytes are
  call targets. Routines up to 0x4432 sit at the same address in both (data handler 0x3E21, sum 0x4218).
  Erase routine: 0x6C01 -> 0x6C60 with its neighbours at unchanged distances. Flash init 0x5296 -> 0x51E4
  is 178 bytes shorter in E103 (E109 accepts flash makers EF, C8, C2 and sizes 11, 13, 14, 16).
  The page-program routine's address in E103 is not known (called only from the unreadable part).
- Vendor tool: PD is updated before the MCU (dialog state 5 before state 0x1B in FUN_00405560), so its
  TI write always runs on the firmware the dock already has. Reply handling FUN_0041a520: any reply whose
  byte 1 is 3B sends the next frame; 09 and 0A need reply byte 5 = 0; 0C is polled every 30 ms until
  byte 5 = 0; 0D is compared with the bundle's sum; then 0E. Last page goes out as 59+59+14.
- `tools/tiwrite.swift` final build: `bin/tiwrite`. `tools/rehearse.sh bin/tiwrite` runs the whole write
  sequence against a model of the chip (12 scenarios: lost acknowledgement, failed erase, skipped page,
  wrong byte, wrong starting content, way back): all end as expected.

## Run sheet for the write (steps 1-3 run on 2026-11-25 17:30-17:37 after the owner's "go")

Conditions: dock on its own power supply, connected and working; nobody unplugs the power supply until
"WRITE VERIFIED" (USB-C unplug is recoverable, a power loss with an empty flash is not).
All commands from `gw-updater/`.

1. `bin/tiwrite preflight itec-ti-flash-original.bin` -> must print sum 7943, "flash already holds this image".
2. `nohup caffeinate -dims bin/tiwrite write itec-ti-flash-patched-trysrc0.bin --expect-current 7943 --yes-erase-ti-flash >> tiwrite-run.log 2>&1`
   in the background; no pipe on the output (a closed pipe would kill the tool). Expect two erases, 3295
   frames, about 43 verify sums, then "WRITE VERIFIED". Estimated 3 to 6 minutes.
3. `bin/tisession sums postwrite-lengths.txt postwrite-sums.csv`, then
   `python3 -I tools/exact_bytes.py postwrite-sums.csv itec-ti-flash-patched-trysrc0.bin` -> 0 differ.
   (Not named `itec-ti-sums-*`: `analyze_pending.py` reads every file of that name as pre-write data.)
4. Owner: unplug USB-C from the Mac, unplug the dock's power for 15 s, power on, wait 10 s, plug into the Mac.
   The TI controller may stay alive on the Mac's VBUS, so the cable must be out during the power cycle.
5. Capture into new files, from `dock-capture/`, both in the background:
   `python3 sampler.py 0.25 >> sampler-patched.tsv 2>> sampler.err` and
   `/usr/bin/log stream --style compact --level info --predicate '(process == "kernel" AND (sender CONTAINS[c] "usb" OR sender CONTAINS "HPM" OR sender CONTAINS[c] "typec" OR sender CONTAINS "Accessory" OR sender CONTAINS "IOPort" OR sender CONTAINS "Thunderbolt" OR sender CONTAINS "SMC" OR eventMessage CONTAINS[c] "usb")) OR process == "powerd" OR process == "IOAccessoryManager" OR process == "accessoryd" OR process == "usbd" OR process BEGINSWITH "DisplayLink"' >> syslog-patched.log 2>&1`.
   Then 10 or more replugs at the Mac end and `python3 replugs.py HH:MM:SS C sampler-patched.tsv`.
   Baseline before the patch: about 13 of 39 fresh attaches connect (5/17, 4/7, about 4/15).
   Also note per attach whether the Mac charges (pmset: 85 W) and which side was source first
   (status register bit 5: 1 = Mac is source).
6. Way back: step 2 with `itec-ti-flash-original.bin` and `--expect-current 78C9`.

Result of the run (log: `tiwrite-run.log`): present sum 7943; each chip erase 3.4 s; flush frame and all
3295 frames acknowledged, 58 s; all prefix sums and the total matched on the first attempt; "WRITE
VERIFIED". Post-write exact bytes: 0x2804 `1e 27 ad c5`, 0x28F9 `08`, 0x16804 `bc 9d d1 03`, 0x168F9 `0c`,
pointers `00 20 00 00` / `00 00 00 00` / `00 20 00 00` / `00 40 01 00`. The dock stayed connected at 85 W
throughout. So the data handler and page program of the dock's E103 firmware behave like the E109 code.
The flash now holds sum 78C9 over 0x29284 bytes.

Steps 4-5 (17:37-18:32): after the power cycle 11 of 11 fresh attaches connected (before: about 13 of 39).
Seven started with the Mac as source and changed to sink about 0.5 s later without a detach; one of those
settled at 60 W instead of 85 W (Mac status connection state 6, cable marker not seen). 44 min stable.
Details: `../REPORT.md`, "Test 10".

If the tool ends with NOT VERIFIED: keep the dock powered, do not power-cycle, read `tiwrite-run.log`
(it logs unexpected replies), run step 1 to see the flash state, and rerun step 2 with `--expect-current any`.
