# CADUAL4KDOCKPD plug-in capture, 2026-11-24 (10:17-13:16)

Host: MacBook Pro Mac15,11, Apple M3 Max, macOS 27.2.1 (26B131), on battery, no other adapter.
Dock: own 100W brick on the DC input, 2x HP 1080p monitors on HDMI, keyboard, mouse (removed from test 4 on).
Capture: ioreg/pmset sampler at 4 Hz (`sampler.tsv`, 24,507 samples), `log stream` of kernel AppleHPM /
IOUSBHostFamily / IOAccessoryManager / AppleSMC plus powerd (`syslog.log`), markers in `markers.log`.
Everything is the Mac's view only.

## Conclusion

The failure is decided at the USB-C attach, before USB and before any PD message.

- Each fresh physical plug-in (or dock power-up) either works at once or enters an endless attach/detach
  loop. With the new cable and the dock powered, 5 of 17 fresh attaches worked (29%). Which end was
  plugged, which Mac port, and which way up made no consistent difference.
- In the loop the dock is seen for about 150 ms, then the plug detection itself is lost; retry every
  ~0.85 s. It never recovers by itself (longest run 30 min, about 2,090 attempts). No hub or DisplayLink
  device enumerates, no PD message reaches the Mac, `pmset` shows no adapter.
- The Mac's port controller status register separates the two outcomes without exception: working =
  0x100cb25d (power-role bit 0, Mac is sink, 3,870 samples); looping = 0x100000ef/ff (power-role bit 1,
  Mac is source, about 1,060 samples). Likely mechanism: both ends are dual-role, the role is settled by
  timing at contact, and when the Mac becomes source the brick-powered dock drops off as soon as VBUS is
  applied. The role bit is decoded with the TI register layout, not Apple documentation.

- Updating both hubs from 93.05 to 93.92 did not remove the failure: after the flash, 4 of 7 replugs
  connected and 3 looped, with the same loop signature. See "After the flash".

Of A-D: A is the nearest (medium-high confidence), reworded as "the Type-C attach/power-role resolution
keeps restarting; the hubs never get as far as reacting". B, C and D are ruled out for this failure
(high confidence): hub counts are 0 throughout a loop, USB2 and USB3 go together with the plug detection,
and two orientations and two Mac ports with separate controllers behave identically.

## Timeline

| Test | Time | What the Mac saw |
|---|---|---|
| 1. Normal setup, old cable, left rear port | 10:25-10:41 | 7 plug-ins: plug detected on CC (status 0x10000009), nothing else for 62-147 s each. No flapping. |
| 1b. Dock off/on, cable in | 10:42-10:44 | No change. |
| 1c. Old cable replugged at dock end | 10:46-10:48 | No change: Mac was seeing only the cable. |
| 1d. New HP cable | 10:53:03 | Loop starts: 11-12 attaches per 10 s, 0 USB devices. |
| 2. Brick removed, cable in | 10:55:39 | Loop stops within one cycle; Mac sees only the cable. Dock does not run from bus power. |
| Brick restored, cable in | 10:57:38 | Loop resumes. |
| 3a. Plug flipped at Mac | 10:59:12 | Orientation 1 -> 2, loop unchanged. |
| 3b. Left front port | 11:00:10 | Port-USB-C@2 / usb-drd1, loop unchanged. |
| 4. Monitors, keyboard, mouse removed | 11:01-11:02 | Loop unchanged. |
| Plug flipped at dock end | 11:04:19 | Connects: SOP attach, 20 V / 4.25 A, 2 + 2 GL3523 hubs, DisplayLink. Stable 147 s. |
| 5. Brick out / in on stable dock | 11:06:49 / 11:07:11 | Out: all USB and PD gone within 0.25 s. In: loop, no recovery. |
| 6a. Reseat at dock, same way | 11:10:58 | Loop. |
| 6b. Flip at dock | 11:13:40 | Connects first attempt. Stable 12.5 min (incl. the hub dumps). |
| 6c. Reseat at dock, same way | 11:26:52 | Loop for 30 min. |
| Accidental replug at Mac, nothing flipped | 11:56:38 | Connects first attempt. |
| 7. Seven replugs at Mac, same port and way | 11:59-12:01 | Connect, loop, connect, loop, loop, loop, loop. |
| Hubs flashed 93.05 -> 93.92 | 12:04-12:08 | Read-back 93.92 on both; dock then stable 85 W for 64 min untouched. |
| 8. Seven replugs at Mac, left rear port, same way | 13:12-13:15 | Connect, connect, loop, loop, loop, connect, connect. |

Totals (to 12:02): about 3,280 attaches recorded on the two ports; 0 hard resets, 0 overcurrent events.
Loop timing (n = 260): attach-to-attach median 868 ms, attach to VBUS-lost median 152 ms (max 162).

## Log lines

One loop cycle (repeats unchanged):
```
10:53:03.933 (AppleHPM) AppleHPMInterface::printConnectedTransports(@0xc) - setting USB2 USB3 as DFP ... as connected
10:53:03.990 (IOUSBHostFamily) usb-drd0-port-hs@00100000: AppleUSBHostPort::cableChangeOccurred: powering on
10:53:04.075 (AppleSMC) (AppleSMCChargerUtil): D1ST is 0! dock:1!
10:53:04.084 (AppleHPM) AppleHPMLDCM::setVbusPresent - called setting 0
10:53:04.089 (AppleHPM) AppleHPMInterface::setCurrentModeFlags(@0xc) - No plug detected, all flags cleared
10:53:04.404 (IOUSBHostFamily) usb-drd0-port-ss@00200000: AppleUSBHostPort::cableChangeOccurred: powering off
```
A working attach:
```
11:04:19.639 (AppleHPM) AppleHPMInterface::processModeFlags(@0xa): Attaching and starting SOP
11:04:19.789 (AppleSMC) (AppleSMCChargerUtil): (port:1) setWinningPowerSourceOption Type:2 V:20000 I:4250
11:04:19.819 (IOUSBHostFamily) usb-drd1-port-ss@01200000: ... enumerated 0x05e3/0620/9305 (USB3.1 Hub / 1) at 5 Gbps
11:04:20.014 (IOUSBHostFamily) usb-drd1-port-hs@01100000: ... enumerated 0x05e3/0610/9305 (USB2.1 Hub / 2) at 480 Mbps
11:04:20.049 (IOUSBHostFamily) AppleUSB30HubPort@01210000: ... enumerated 0x17e9/6000/3201 (USB-C Dual-4K Dock / 3) at 5 Gbps
```
"Attaching and starting SOP" never appears in a loop cycle.

## Test 5 prediction

Half held. On a stable dock, pulling the brick dropped every USB device together within one sample. They
did not come back when the brick returned: the dock re-attached into the loop. Caveat: the dock is dead
without its brick, so "everything drops" is also what plain power loss looks like; this does not confirm
the firmware's local-power-change reboot path specifically.

## Hub firmware on this unit was 93.05, not 93.92

- ioreg bcdDevice 37637 (0x9305), kernel enumeration lines "/9305", fwupd "93.5" on both hubs.
- Images dumped read-only with `fwupdtool firmware-dump` (64 KiB each, code to 0x6000, built 2018-05-10):
  `hub-HUB_16-9305.bin` sha256 f411e865...c7aaaa, `hub-HUB_00-9305.bin` sha256 20fbe015...b89b3c.
- The firmware findings in the investigation describe 93.92 and may not hold for what runs here.
- fwupd 2.1.10 has no size for flash ID c8 40 10; a temporary quirk (FirmwareSizeMax = 0x10000) was added
  for the dump and removed afterwards.

## After the flash (93.92)

- Both hubs flashed with `fwupdtool install-blob` from the vendor ROMs (SHA-256 matched the pinned values).
  Both runs ended "error resetting device: USB error" after erase/write/verify; read-back shows 93.92 in
  fwupd and bcdDevice 37778 (0x9392) on all four hub halves; kernel lines now say "/9392".
- Once connected, the dock stayed up 64 min with no drop (12:08-13:12).
- Test 8: 7 Mac-end replugs, 4 connected (57%), 3 looped. Before the flash: 5 of 17 (29%). The difference
  is not significant at these counts (Fisher exact p about 0.36); read it as "unchanged or somewhat better,
  not fixed".
- Loop signature is identical to 93.05: status 0x100000ff (Mac = source), 0 USB devices, 14-15 "No plug
  detected" per ~12 s, no "Attaching and starting SOP". Working attaches: 0x100cb25d (Mac = sink), 8
  devices in about 1.5 s. The status register again split the outcomes without exception.
- Since no hub enumerates during a loop, hub firmware is not expected to matter there; this result agrees.

## Reference signatures (from fwupd resetting the hubs, PD steady at 85 W)

- First hub reset: both hub pairs and DisplayLink drop, back in about 5 s. This is what B would look like.
- Second hub reset: one USB3 hub drops, back in about 1 s; first hub and DisplayLink stay. This is D.
Neither resembles the loop.

## Fits none of A-D

- The old cable produced a third state: Mac sees the cable's marker but no dock, with the dock on, off, or
  unplugged from the cable. An unpowered dock on the new cable gives the identical state, so the old cable
  may be faulty or the dock may simply not have presented; not resolved.
- Dock PD controller identifies as Texas Instruments (SOP VID 0x0451) with placeholder PID 0x1234. The dock
  also contains VIA hubs (2109:0210 / 2210) and a "Good way DUD15X0D1" device (065f:f60f).
- `AppleSMCChargerUtil: D1ST is 0! dock:1!` in every loop cycle; meaning unknown.

## Could not observe

- The dock's side: CC terminations, PD controller state, rails. No PD analyser; the TI controller is not
  reachable over USB (fwupd `ti_tps6598x` and `vli` plugins found nothing).
- The status register decoding is assumed from the TI layout. The split between working and looping
  values is measured; the meaning of the bits is not.
- Why the automatic retries always lose while a physical replug sometimes wins.
- Whether 29% (93.05) or 57% (93.92) holds in general: 17 and 7 attaches, one cable, one Mac, one morning. No second host, no second
  brick, right-hand Mac port untested, old cable not retried after 10:50.
- When the dock last worked reliably and whether anything changed since; asked, not answered.
- Capture limits: ioreg PD counters lag by minutes (totals only); 35 s sampler gap around 10:45; the
  dock was reset 5 times by fwupd between 11:21:13 and 11:23:07.
