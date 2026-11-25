#!/usr/bin/env python3
"""Read-only dock sampler: one TSV line per sample (~4/s) to stdout.
Fields: time, usb2 hubs (05e3:0610), usb3 hubs (05e3:0620), DisplayLink present,
total USB devices, pmset adapter, battery ExternalConnected/watts,
per-port HPM state, per-port PD counters."""
import re, subprocess, sys, time
from datetime import datetime

def run(*cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=5).stdout
    except Exception as e:  # keep sampling through a slow ioreg
        return f"ERR {e}"

def usb():
    # split the IOUSB plane into device blocks, read vendor+product per block
    hubs2 = hubs3 = dl = total = 0
    for blk in re.split(r"\n(?=[ |]*\+-o )", run("ioreg", "-p", "IOUSB", "-l", "-w0")):
        v = re.search(r'"idVendor" = (\d+)', blk)
        p = re.search(r'"idProduct" = (\d+)', blk)
        if not (v and p):
            continue
        total += 1
        v, p = int(v[1]), int(p[1])
        hubs2 += (v, p) == (0x05E3, 0x0610)
        hubs3 += (v, p) == (0x05E3, 0x0620)
        dl += v == 0x17E9
    return f"usb2hub={hubs2}\tusb3hub={hubs3}\tdisplaylink={dl}\tusbdev={total}"

PORT_KEYS = ["ConnectionActive", "PlugOrientation", "TransportsActive",
             "IOAccessoryUSBSuperSpeedActive", "IOAccessoryUSBConnectString",
             "IOAccessoryPowerCurrentLimits", "HPDAsserted", "ConnectionCount",
             "Plug Event Count", "Overcurrent Count"]

def ports():
    out = []
    txt = run("ioreg", "-r", "-c", "AppleHPMInterfaceType10", "-l", "-w0") + \
          run("ioreg", "-r", "-c", "AppleHPMInterfaceType11", "-l", "-w0")
    for blk in re.split(r"\n(?=\+-o Port-)", txt):
        name = re.match(r"\s*\+-o (Port-[^ ]+( \d)?@\d)", blk)
        if not name:
            continue
        top = blk.split("\n  +-o ")[0]  # port node only, not child transports
        vals = []
        for k in PORT_KEYS:
            m = re.search(rf'"{re.escape(k)}" = (.*)', top)
            vals.append(f"{k.replace(' ', '')}={m[1] if m else '?'}")
        out.append(name[1].replace(" ", "") + "[" + " ".join(vals) + "]")
    return " ".join(out)

PD_KEYS = ["AttachCount", "DetachCount", "HardResetCount", "IrqCntHrdRst",
           "IrqCntRxSrcCap", "IrqCntPdStsUpd", "IrqCntPwrStsUpd", "IrqCntStsUpd",
           "PwrRoleSwapCount", "DataRoleSwapCount", "ActiveContractRdo",
           "PDst", "PowerState", "MaxPower", "FetStatus", "ShortDetectCount"]

def battery():
    txt = run("ioreg", "-r", "-c", "AppleSmartBattery", "-w0")
    ext = re.search(r'"ExternalConnected" = (\w+)', txt)
    watts = re.search(r'"AdapterDetails" = \{[^\n]*?"Watts"=(\d+)', txt)
    volt = re.search(r'"AdapterDetails" = \{[^\n]*?"AdapterVoltage"=(\d+)', txt)
    pci = re.search(r'"PortControllerInfo" = \((.*)', txt)
    pcs = []
    for i, blk in enumerate(pci[1].split("},{") if pci else []):
        vals = []
        for k in PD_KEYS:
            m = re.search(rf'"PortController{k}"=(-?\d+)', blk)
            vals.append(f"{k}={m[1] if m else '?'}")
        pcs.append(f"pc{i}[" + " ".join(vals) + "]")
    return (f"ext={ext[1] if ext else '?'} W={watts[1] if watts else '-'} "
            f"mV={volt[1] if volt else '-'}\t" + " ".join(pcs))

def status():
    # port controller status register per HAL node (@C = Port-USB-C@1 on this Mac)
    txt = run("ioreg", "-r", "-c", "AppleHPMDeviceHALType3", "-l", "-w0")
    pairs = re.findall(r'^\+-o AppleHPMDeviceHALType3(@\w+).*?^  \|? *"Status Reg" = (\d+)', txt, re.M | re.S)
    return "hpm[" + " ".join(f"StatusReg{a}=0x{int(v):08x}" for a, v in pairs) + "]"

def adapter():
    return "pmset=" + " ".join(run("pmset", "-g", "adapter").split())

if __name__ == "__main__":
    interval = float(sys.argv[1]) if len(sys.argv) > 1 else 0.25
    while True:
        t0 = time.time()
        ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]
        print("\t".join([ts, usb(), adapter(), battery(), ports(), status()]), flush=True)
        time.sleep(max(0, interval - (time.time() - t0)))
