#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A TCP relay for scripts/accept.sh: listen PORT -> 127.0.0.1:TARGET. RATE (bytes per second, 0 = unlimited) slows the answers of the
server down, so that a download takes seconds and the script can cut it in the middle by killing the relay."""
import socket
import sys
import threading
import time


def pump(source, sink, rate):
    try:
        while True:
            data = source.recv(4096 if rate else 65536)
            if not data:
                break
            sink.sendall(data)
            if rate:
                time.sleep(len(data) / rate)
    except OSError:
        pass
    finally:
        for s in (source, sink):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def main():
    port, target, rate = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", port))
    listener.listen(64)
    while True:
        client, _ = listener.accept()
        try:
            upstream = socket.create_connection(("127.0.0.1", target))
        except OSError:
            client.close()
            continue
        threading.Thread(target=pump, args=(client, upstream, 0), daemon=True).start()
        threading.Thread(target=pump, args=(upstream, client, rate), daemon=True).start()


main()
