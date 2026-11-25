#!/usr/bin/env python3
"""Minimal 8051 linear disassembler. Usage: d51.py file base start end"""
import sys
R = [f"R{i}" for i in range(8)]
def ops():
    t = {}
    def s(op, fmt, n=1): t[op] = (fmt, n)
    s(0x00,"NOP"); s(0x02,"LJMP {a16}",3); s(0x12,"LCALL {a16}",3); s(0x22,"RET"); s(0x32,"RETI")
    s(0x03,"RR A"); s(0x13,"RRC A"); s(0x23,"RL A"); s(0x33,"RLC A")
    for i in range(8):
        s(0x01+i*0x20,"AJMP {a11}",2); s(0x11+i*0x20,"ACALL {a11}",2)
    for b,m in ((0x00,"INC"),(0x10,"DEC")):
        s(b+4,m+" A"); s(b+5,m+" {d}",2); s(b+6,m+" @R0"); s(b+7,m+" @R1")
        for i in range(8): s(b+8+i,m+" "+R[i])
    for b,m in ((0x20,"ADD"),(0x30,"ADDC"),(0x40,"ORL"),(0x50,"ANL"),(0x60,"XRL"),(0x90,"SUBB")):
        s(b+4,m+" A,#{i}",2); s(b+5,m+" A,{d}",2); s(b+6,m+" A,@R0"); s(b+7,m+" A,@R1")
        for i in range(8): s(b+8+i,m+" A,"+R[i])
    for b,m in ((0x40,"ORL"),(0x50,"ANL"),(0x60,"XRL")):
        s(b+2,m+" {d},A",2); s(b+3,m+" {d},#{i}",3)
    s(0x10,"JBC {b},{r}",3); s(0x20,"JB {b},{r}",3); s(0x30,"JNB {b},{r}",3)
    s(0x40,"JC {r}",2); s(0x50,"JNC {r}",2); s(0x60,"JZ {r}",2); s(0x70,"JNZ {r}",2); s(0x80,"SJMP {r}",2)
    s(0x72,"ORL C,{b}",2); s(0x82,"ANL C,{b}",2); s(0xa0,"ORL C,/{b}",2); s(0xb0,"ANL C,/{b}",2)
    s(0x73,"JMP @A+DPTR"); s(0x83,"MOVC A,@A+PC"); s(0x93,"MOVC A,@A+DPTR"); s(0x84,"DIV AB"); s(0xa4,"MUL AB")
    s(0x74,"MOV A,#{i}",2); s(0x75,"MOV {d},#{i}",3); s(0x76,"MOV @R0,#{i}",2); s(0x77,"MOV @R1,#{i}",2)
    for i in range(8):
        s(0x78+i,"MOV "+R[i]+",#{i}",2); s(0x88+i,"MOV {d},"+R[i],2); s(0xa8+i,"MOV "+R[i]+",{d}",2)
        s(0xe8+i,"MOV A,"+R[i]); s(0xf8+i,"MOV "+R[i]+",A"); s(0xc8+i,"XCH A,"+R[i])
        s(0xb8+i,"CJNE "+R[i]+",#{i},{r}",3); s(0xd8+i,"DJNZ "+R[i]+",{r}",2)
    s(0x85,"MOV {d2},{d}",3); s(0x86,"MOV {d},@R0",2); s(0x87,"MOV {d},@R1",2)
    s(0x90,"MOV DPTR,#{a16}",3); s(0x92,"MOV {b},C",2); s(0xa2,"MOV C,{b}",2); s(0xa3,"INC DPTR"); s(0xa5,"DB A5")
    s(0xa6,"MOV @R0,{d}",2); s(0xa7,"MOV @R1,{d}",2); s(0xb2,"CPL {b}",2); s(0xb3,"CPL C")
    s(0xb4,"CJNE A,#{i},{r}",3); s(0xb5,"CJNE A,{d},{r}",3); s(0xb6,"CJNE @R0,#{i},{r}",3); s(0xb7,"CJNE @R1,#{i},{r}",3)
    s(0xc0,"PUSH {d}",2); s(0xd0,"POP {d}",2); s(0xc2,"CLR {b}",2); s(0xc3,"CLR C"); s(0xd2,"SETB {b}",2); s(0xd3,"SETB C")
    s(0xc4,"SWAP A"); s(0xd4,"DA A"); s(0xe4,"CLR A"); s(0xf4,"CPL A"); s(0xc5,"XCH A,{d}",2); s(0xc6,"XCH A,@R0"); s(0xc7,"XCH A,@R1")
    s(0xd5,"DJNZ {d},{r}",3); s(0xd6,"XCHD A,@R0"); s(0xd7,"XCHD A,@R1")
    s(0xe0,"MOVX A,@DPTR"); s(0xe2,"MOVX A,@R0"); s(0xe3,"MOVX A,@R1"); s(0xf0,"MOVX @DPTR,A"); s(0xf2,"MOVX @R0,A"); s(0xf3,"MOVX @R1,A")
    s(0xe5,"MOV A,{d}",2); s(0xe6,"MOV A,@R0"); s(0xe7,"MOV A,@R1"); s(0xf5,"MOV {d},A",2); s(0xf6,"MOV @R0,A"); s(0xf7,"MOV @R1,A")
    return t
T = ops()
SFR = {0x81:"SP",0x82:"DPL",0x83:"DPH",0xe0:"ACC",0xf0:"B",0xd0:"PSW",0x96:"USB0ADR",0x97:"USB0DAT",0xa8:"IE"}
def dis(b, base, start, end):
    pc = start
    while pc < end:
        o = pc - base; op = b[o]; fmt, n = T.get(op, (f"DB {op:02X}", 1)); a = b[o+1:o+n]
        nxt = pc + n; f = {}
        if "{a16}" in fmt: f["a16"] = f"0x{a[0]<<8|a[1]:04X}"
        if "{a11}" in fmt: f["a11"] = f"0x{(nxt & 0xF800)|((op>>5)<<8)|a[0]:04X}"
        if "{r}" in fmt: r = a[-1]; f["r"] = f"0x{(nxt + (r - 256 if r > 127 else r)) & 0xFFFF:04X}"
        dn = lambda x: SFR.get(x, f"{x:02X}h")
        if op == 0x85: f["d"] = dn(a[0]); f["d2"] = dn(a[1])
        elif "{d}" in fmt: f["d"] = dn(a[0])
        if "{b}" in fmt: f["b"] = f"bit{a[0]:02X}"
        if "{i}" in fmt:
            iv = a[1] if (op in (0x43,0x53,0x63,0x75)) else (a[0] if n >= 2 else 0)
            if op in (0xb4,0xb6,0xb7) or 0xb8 <= op <= 0xbf: iv = a[0]
            f["i"] = f"0x{iv:02X}"
        print(f"{pc:04X}: {b[o:o+n].hex():<8} {fmt.format(**f)}")
        pc = nxt
if __name__ == "__main__":
    b = open(sys.argv[1], "rb").read()
    dis(b, int(sys.argv[2], 16), int(sys.argv[3], 16), int(sys.argv[4], 16))
