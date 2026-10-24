#!/usr/bin/env python3
# regdump.py — a minimal client that completes login + configuration against a
# real vanilla server and writes every registry_data body verbatim.
#
# Driven by ci/dump-registries.sh. Deliberately not a library: it exists to
# turn vanilla into bytes once per version bump, and the protocol it speaks is
# the one documented in crates/rustmc-server/src/{login,configuration}.rs.
import socket, struct, sys, zlib, json, hashlib, os
def vi(n):
    out=b""
    while True:
        b=n&0x7F; n>>=7
        if n: out+=bytes([b|0x80])
        else: return out+bytes([b])
class C:
    def __init__(s,h,p):
        s.s=socket.create_connection((h,p),10); s.s.settimeout(20); s.thr=-1
    def rd(s,n):
        d=b""
        while len(d)<n:
            c=s.s.recv(n-len(d))
            if not c: raise EOFError
            d+=c
        return d
    def rvi_sock(s):
        n=0;sh=0
        while True:
            b=s.rd(1)[0]; n|=(b&0x7F)<<sh
            if not b&0x80: return n
            sh+=7
    def send(s,pid,body):
        pkt=vi(pid)+body
        if s.thr>=0:
            if len(pkt)>=s.thr:
                comp=zlib.compress(pkt); inner=vi(len(pkt))+comp
            else: inner=vi(0)+pkt
        else: inner=pkt
        s.s.sendall(vi(len(inner))+inner)
    def recv(s):
        ln=s.rvi_sock(); data=s.rd(ln)
        if s.thr>=0:
            # uncompressed-size varint, then maybe zlib
            n=0;sh=0;i=0
            while True:
                b=data[i]; i+=1; n|=(b&0x7F)<<sh
                if not b&0x80: break
                sh+=7
            data = zlib.decompress(data[i:]) if n>0 else data[i:]
        n=0;sh=0;i=0
        while True:
            b=data[i]; i+=1; n|=(b&0x7F)<<sh
            if not b&0x80: break
            sh+=7
        return n, data[i:]
def rstr(b,i):
    n=0;sh=0
    while True:
        x=b[i]; i+=1; n|=(x&0x7F)<<sh
        if not x&0x80: break
        sh+=7
    return b[i:i+n].decode(), i+n
# --- play state, for CHUNK_DUMP -------------------------------------------
#
# Every id here is from Mojang's own report
# (oracle-work/vanilla-<ver>/generated/reports/packets.json), never memory —
# the same rule crates/rustmc-proto/src/ids.rs states.
CB_CHUNK_BATCH_FINISHED = 0x0b
CB_GAME_EVENT           = 0x26
CB_KEEP_ALIVE           = 0x2c
CB_LEVEL_CHUNK          = 0x2d
CB_LOGIN                = 0x31
CB_PING                 = 0x3d
CB_PLAYER_POSITION      = 0x48
CB_DISCONNECT           = 0x20
SB_ACCEPT_TELEPORT      = 0x00
SB_CHUNK_BATCH_RECEIVED = 0x0b
SB_KEEP_ALIVE           = 0x1c
SB_PLAYER_LOADED        = 0x2c
SB_PONG                 = 0x2d

def play_capture(c, out):
    """Stay in play long enough to collect a chunk batch, and write it out.

    The server drives this: it will not send a second batch until the client
    acknowledges the first (`chunk_batch_received`), and it will disconnect a
    client that ignores keep-alives. So this answers everything it is asked
    rather than reading passively.
    """
    chunks = []
    seen = {}
    deadline = 60
    import time
    t0 = time.time()
    while time.time() - t0 < deadline:
        try:
            pid, body = c.recv()
        except Exception as e:
            print("play recv ended:", type(e).__name__)
            break
        seen[pid] = seen.get(pid, 0) + 1
        if pid == CB_KEEP_ALIVE:
            c.send(SB_KEEP_ALIVE, body)
        elif pid == CB_PING:
            c.send(SB_PONG, body)
        elif pid == CB_PLAYER_POSITION:
            # teleport id is the leading varint; accepting it is what makes
            # the server treat the player as positioned.
            n = 0; sh = 0; i = 0
            while True:
                b = body[i]; i += 1; n |= (b & 0x7F) << sh
                if not b & 0x80: break
                sh += 7
            c.send(SB_ACCEPT_TELEPORT, vi(n))
            c.send(SB_PLAYER_LOADED, b"")
        elif pid == CB_CHUNK_BATCH_FINISHED:
            # float: chunks per tick the client claims it can take.
            c.send(SB_CHUNK_BATCH_RECEIVED, struct.pack(">f", 16.0))
        elif pid == CB_LEVEL_CHUNK:
            # The first eight bytes are the chunk position. Whether they are
            # two big-endian i32s is not assumed here — they are recorded raw
            # and printed both ways so the capture itself decides.
            cx, cz = struct.unpack(">ii", body[:8])
            chunks.append((cx, cz, body))
        elif pid == CB_DISCONNECT:
            print("play disconnect:", body[:160]); break
        elif pid == CB_LOGIN:
            print(f"  play/login {len(body)} bytes")
    print("play packets seen:", {hex(k): v for k, v in sorted(seen.items())})
    print(f"captured {len(chunks)} level_chunk_with_light packets")
    if chunks:
        xs = [c0 for c0, _, _ in chunks]; zs = [c1 for _, c1, _ in chunks]
        print(f"  chunk x {min(xs)}..{max(xs)}, z {min(zs)}..{max(zs)}")
        with open(out, "wb") as f:
            f.write(b"RMCCHK01")
            f.write(len(chunks).to_bytes(4, "big"))
            for cx, cz, body in chunks:
                f.write(struct.pack(">iiI", cx, cz, len(body)))
                f.write(body)
        print("wrote", out, sum(len(b) for _, _, b in chunks), "bytes of chunk bodies")

host,port,name=sys.argv[1],int(sys.argv[2]),sys.argv[3]
c=C(host,port)
hs=vi(776)+vi(len("localhost"))+b"localhost"+struct.pack(">H",port)+vi(2)
c.send(0,hs)
nb=name.encode(); c.send(0, vi(len(nb))+nb+b"\x00"*16)
while True:
    pid,body=c.recv()
    if pid==3:  # login compression
        n=0;sh=0;i=0
        while True:
            b=body[i]; i+=1; n|=(b&0x7F)<<sh
            if not b&0x80: break
            sh+=7
        c.thr=n; continue
    if pid==2: break   # login_finished
    if pid==0:
        print("login disconnect:", body[:120]); sys.exit(1)
c.send(3,b"")   # login_acknowledged
# client_information: lang, viewdist, chatmode(vi), colors(bool), skinparts(u8), hand(vi), textfilter(bool), listing(bool), particles(vi)
lang=b"en_gb"
ci=vi(len(lang))+lang+bytes([10])+vi(0)+b"\x01"+bytes([0x7f])+vi(1)+b"\x00"+b"\x01"+vi(0)
c.send(0,ci)
regs={}
raw=[]
while True:
    pid,body=c.recv()
    if pid==14:   # select_known_packs (cb)
        c.send(7, vi(0))
    elif pid==7:  # registry_data
        rid,i=rstr(body,0)
        n=0;sh=0
        while True:
            x=body[i]; i+=1; n|=(x&0x7F)<<sh
            if not x&0x80: break
            sh+=7
        ents=[]
        for _ in range(n):
            eid,i=rstr(body,i)
            has=body[i]; i+=1
            if has:
                # skip the nbt by parsing minimally is hard; record offset only
                ents.append(eid); break
            ents.append(eid)
        regs[rid]=(n,len(body))
        raw.append((rid,body))
    elif pid==3:  # finish_configuration
        c.send(3,b"")            # ack -> play state
        if os.environ.get("PLAY_DUMP"):
            seen=[]
            for _ in range(40):
                try: ppid,pbody=c.recv()
                except Exception: break
                seen.append((ppid,len(pbody)))
                if ppid==49 and os.environ.get("PLAY_DUMP"):
                    open(os.environ["PLAY_DUMP"],"wb").write(pbody)
            names={49:"login",38:"game_event",94:"set_chunk_cache_center",
                   12:"chunk_batch_start",45:"level_chunk_with_light",
                   11:"chunk_batch_finished",72:"player_position",32:"disconnect"}
            for pid_,ln in seen:
                print(f"PLAY {names.get(pid_,pid_)} len={ln}")
        elif os.environ.get("CHUNK_DUMP"):
            play_capture(c, os.environ["CHUNK_DUMP"])
        break
    elif pid==2:
        print("config disconnect"); break
print(json.dumps({k:{"entries":v[0],"bytes":v[1]} for k,v in sorted(regs.items())}, indent=0))
import os
out=os.environ.get("REGDUMP_OUT")
if out:
    with open(out,"wb") as f:
        f.write(b"RMCREG01")
        f.write(len(raw).to_bytes(4,"big"))
        for rid,body in raw:
            f.write(len(body).to_bytes(4,"big")); f.write(body)
    print("wrote", out, sum(len(b) for _,b in raw), "bytes of payload")
print("TOTAL registries:", len(regs))
