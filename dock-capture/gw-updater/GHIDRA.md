# Opening the files in Ghidra

All paths are relative to `dock-capture/`. Values marked (measured) were computed from the files;
values marked (assumed) are best guesses to correct once the listing looks wrong.

Goal of the work: find the HID command format of the Good Way device (USB 065f:f60f), then the harmless
queries (status, version, dump) before anything is sent to the dock.

## Priority order

| # | File | Why |
|---|---|---|
| 1 | `gw-updater/seg0-mcu-efm8-app.bin` | Firmware of the chip we would talk to. Holds the command handler. |
| 2 | `~/Downloads/DUD15X0D1_Update_E108.exe` | The PC side of the same protocol. Easier to read than 8051. |
| 3 | `gw-updater/seg5-ti-tps65982-image.bin` | A TI PD controller image, for format reference only. |
| 4 | `gw-updater/itec-mcu-code-02AA-3BA9.bin` | Code read live from YOUR dock's management chip (bootloader + start of app). |
| 5 | `hub-HUB_16-9305.bin`, `hub-HUB_00-9305.bin`, the two 93.92 ROMs | GL3523 hubs. Lower priority now. |

Skip `seg1`, `seg2`, `seg3`, `seg4`: display and Thunderbolt parts of the Cable Matters dock
(16-bit x86 code for an ST video chip, Thunderbolt NVM). Your dock has neither.

## 1. Management chip firmware: `seg0-mcu-efm8-app.bin` (16,808 bytes)

File > Import File, format **Raw Binary**, then:

- Language: `8051:BE:16:default` (processor "8051", variant "default").
- Options > Block Name `CODE`, **Base Address `0x3000`** (measured: 776 of 1,027 LJMP/LCALL targets land
  inside the image with this base; reset stub at 0x3000 is `LJMP 0x569B`). The first 0x3000 bytes of the
  chip are its bootloader, which is not in this file.
- Let auto-analysis run with defaults. Then press `D` at 0x3000 and 0x3003 if they are not disassembled.

The chip is a Silicon Labs USB 8051 (the firmware names EFM8; it reads SFR 0x96/0x97, which are USB0ADR
and USB0DAT on that family). Ghidra has no EFM8 register names: rename SFRs by hand from the EFM8UB
reference manual as you meet them. Start with 0x96 `USB0ADR`, 0x97 `USB0DAT`.

Where to start (Ghidra address = file offset + 0x3000):

| String | File offset | Ghidra address |
|---|---|---|
| `JUMP ISP` | 0x1127 | 0x4127 |
| `USBUID_ISP_EFM8_CMD_DEVICE_VERSION! %c%x%x%x` | 0x1158 | 0x4158 |
| `TI erase!` | 0x11C7 | 0x41C7 |
| `switch USB to TI` | 0x3858 | 0x6858 |
| `dump %d` | 0x3EB6 | 0x6EB6 |

Use Search > For Strings, then References > Show References to each string. The function that prints
`USBUID_ISP_EFM8_CMD_...` is the HID command dispatcher: its switch on the first payload byte is the
command table. There is also a jump table at 0x3112 (`MOV DPTR,#0x3112` ... `JMP @A+DPTR`).

The device's HID report descriptor (from the live dock): usage page 0xFF00, report ID 1 = 7 bytes,
report ID 2 = 63 bytes, both directions. Expect the handler to branch on report ID first.

## 2. PC tool: `DUD15X0D1_Update_E108.exe` (6.2 MB)

Do not run it. It is a Windows program and carries firmware for a different dock.

- Import normally: Ghidra detects **PE**, `x86:LE:32:default`, compiler `windows`. Accept defaults.
- It is an MFC dialog app (`CGW_IspToolDlg`), built 2020-05-04, no symbols. Analysis takes a few minutes.
- Ghidra loads only the PE sections. The firmware bundle is appended after them, at file offset 0x564400
  (already extracted as `gw-updater/overlay.bin` and the `seg*` files).

Where to start:

- Symbol Tree > Imports (measured, this is the full HID set): `HidD_GetHidGuid`, `HidD_GetAttributes`,
  `HidD_GetPreparsedData`, `HidP_GetCaps`, `CreateFileW`, `WriteFile`, `ReadFile`. There is no
  `HidD_SetOutputReport` or feature call, so every command goes out through `WriteFile` and every answer
  comes back through `ReadFile`. Follow the callers of those two up one or two levels to the function
  that builds the 64-byte buffer: the constants written into it are the commands.
- Strings: `Start updating PD`, `Start updating MCU`, `ISP_GW_WORK_END`, `ISP_GW_WORK_FAIL`.
- Search > Memory for the UTF-16 or ASCII text `065F` / `f60F` and for the bundle header `5A/`.
  The code that parses `5A/E108/2020/05/04/GW/065F/f60F/0003/` tells you what each segment is for.

Match each command constant found here against the dispatcher in file 1. A command that only reads
(version, status) and appears in both is a candidate for the first live query.

## 3. TI image: `seg5-ti-tps65982-image.bin` (168,580 bytes)

This is the Cable Matters dock's TPS65982 image, not yours. Use it to learn the layout.

Layout (measured):

| File offset | Content |
|---|---|
| 0x0000 | Region 0 pointer = 0x2000 |
| 0x1000 | Region 1 pointer = 0x2000 |
| 0x2000 | Boot header, magic `01 00 E0 AC`; header size 0x1000, application size 0x12284 |
| 0x3000 | Application, 0x12284 bytes |
| 0x16000 | Second copy of the same header and application |

To disassemble the application:

- Import as **Raw Binary**, language `ARM:LE:32:Cortex` (Cortex-M, Thumb).
- Options: File Offset `0x3000`, Length `0x12284`, **Base Address `0x0`** (measured: vector table at the
  start reads SP = 0x13FF0, reset = 0xDD, NMI = 0x10C61, HardFault = 0xC151, all inside 0..0x14000).
- After import, mark 0x0 as a table of 4-byte pointers; go to 0xDC and press `F12` (disassemble Thumb).
- Add uninitialised memory blocks as references demand them. Words pointing at 0x2000xxxx and 0x2800xxxx
  are common (assumed: the chip's ROM and peripherals; not in this file).

TI's strings (`Texas Instruments`, `TPS65982`) sit at file offset 0x2839, inside the boot header.

## 4. Your dock's management chip: `itec-mcu-code-02AA-3BA9.bin` (14,592 bytes)

Read from the i-tec dock on 2026-11-25 with command 1B (read only), indices 0-255.

- Raw Binary, `8051:BE:16:default`, **Base Address `0x02AA`** (measured: the bytes at 0x3000 are the app's
  reset stub `02 56 8F`, and the command dispatcher sits at 0x3089-0x3445 exactly as in file 1).
- Content: info table 0x02AA-0x03FF (text, 57-byte records), bootloader code 0x0400-0x2FFF
  (`***BOOT DUD15X0D1 TI E101`, built Jun 25 2018), application 0x3000-0x3BA9 (version E103).
- Missing: 0x0000-0x02A9 (reset vectors, start of bootloader) and the application above 0x3BA9.
- The application part is byte-identical to file 1 except for call targets that moved (84 of 2,986
  bytes). Use file 1 for the application logic and this file for the bootloader.

Bootloader strings to start from: `ISP fail` 0x1B02, `ERASE %d` 0x1B4A, `CHECKSUM! %x` 0x1B55,
`DEVICE STATUS` 0x1B65, `SPARE_AREA: %d` 0x1B75, `addr:%x` 0x1B86.

## 5. GL3523 hub images

Files: `hub-HUB_16-9305.bin`, `hub-HUB_00-9305.bin` (93.05 dumps from the dock, 64 KiB, code in the
first 0x6000 bytes) and `~/Downloads/Patch_CADUAL4KDOCKPD_firmware/roms/Hub_1_9392_L1.ROM`,
`Hub_1_9392_L2.ROM` (vendor 93.92, 24 KiB).

- Raw Binary, `8051:BE:16:default`, **Base Address `0x8000`** (measured: 557 of 884 in-range jump/call
  targets land right after a RET or jump at this base; the next best base scores 109).
  An earlier version of this file said 0x0; that was wrong.
- For the 64 KiB dumps set Length to `0x6000` in the import options; the rest is 0xFF.
- Targets below 0x8000 point into the chip's built-in ROM, which is in none of these files. Expect
  unresolved calls there.
- Reset stub at 0x8000 is `LJMP 0xB757` (93.92) or `LJMP 0xB6CF` (93.05). Strings (`GenesysLogic`,
  `USB2.1 Hub`, `XROM`) are near 0x80F0.
- To see what the vendor patch changed, import a 93.05 dump and the matching 93.92 ROM with the same
  base and use Tools > Version Tracking.

## Not known

- Whether your dock's management chip runs this firmware version (its HID reports version 1.00).
- `seg6` (4,224 bytes, starts `37 37 33 37`), `seg7` (858 bytes of 8051 code, possibly a RAM loader for
  the bootloader stage) and what the tool does with them. `seg8` is a text version table.
- The TI application's ROM. The image is a patch that calls into ROM not present in any file here.
