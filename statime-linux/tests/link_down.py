#!/usr/bin/env python3
"""Linux/root integration check: build statime, then run this with its binary path.

All interface changes occur inside a fresh network namespace. The daemon uses
only a virtual monotonic clock and never exports a clock socket.
"""
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time


def ip(*args):
    subprocess.run(["ip", *args], check=True)


def check(binary, mode):
    ip("link", "add", "ptptest", "type", "dummy")
    try:
        ip("link", "set", "ptptest", "address", "00:11:22:33:44:55")
        ip("addr", "add", "192.0.2.1/24", "dev", "ptptest")
        ip("link", "set", "ptptest", "up")
        with tempfile.TemporaryDirectory(prefix="statime-link-down-") as tmp:
            config = Path(tmp) / "config.toml"
            config.write_text(f'''loglevel = "debug"
virtual-system-clock = true
virtual-system-clock-base = "monotonic_raw"
usrvclock-export = false
[[port]]
interface = "ptptest"
network-mode = "{mode}"
hardware-clock = "none"
protocol-version = "PTPv2"
announce-interval = -2
sync-interval = -2
''')
            with (Path(tmp) / "daemon.log").open("w+") as log:
                daemon = subprocess.Popen([binary, "-c", str(config)], stdout=log, stderr=log)
                try:
                    time.sleep(4)
                    ip("link", "set", "ptptest", "down")
                    time.sleep(3)
                    ip("link", "set", "ptptest", "up")
                    packets = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3))
                    packets.bind(("ptptest", 0))
                    packets.settimeout(0.2)
                    time.sleep(3)
                    recovered = False
                    try:
                        deadline = time.monotonic() + 1
                        while time.monotonic() < deadline:
                            frame = packets.recv(65536)
                            if frame[12:14] == b"\x88\xf7":
                                recovered = True
                            elif frame[12:14] == b"\x08\x00" and len(frame) > 38 and frame[23] == 17:
                                udp = 14 + (frame[14] & 15) * 4
                                if int.from_bytes(frame[udp + 2:udp + 4], "big") in (319, 320):
                                    recovered = True
                    except socket.timeout:
                        pass
                    finally:
                        packets.close()
                    log.flush()
                    log.seek(0)
                    output = log.read()
                    assert daemon.poll() is None, output
                    assert "panicked" not in output, output
                    assert "Failed to send event message" in output, output
                    assert "Failed to send general message" in output, output
                    assert recovered, "no PTP transmission after link recovery\n" + output
                    print(f"{mode}: event/general send failures observed; no panic; PTP packets resume after link up")
                finally:
                    daemon.terminate()
                    daemon.wait(timeout=5)
    finally:
        ip("link", "delete", "ptptest")


if __name__ == "__main__":
    if len(sys.argv) == 2:
        subprocess.run(["unshare", "--net", sys.executable, str(Path(__file__).resolve()),
                        "--inside", str(Path(sys.argv[1]).resolve()),
                        str(os.stat("/proc/self/ns/net").st_ino)], check=True)
    elif len(sys.argv) == 4 and sys.argv[1] == "--inside":
        assert os.stat("/proc/self/ns/net").st_ino != int(sys.argv[3]), "network namespace was not isolated"
        for mode in ("ipv4", "ethernet"):
            check(sys.argv[2], mode)
    else:
        sys.exit(f"usage: {sys.argv[0]} /path/to/statime")
