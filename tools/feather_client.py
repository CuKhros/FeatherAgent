#!/usr/bin/env python3
"""FeatherAgent 前端契约最小参考客户端（无第三方依赖，JSON 模式）。

用法:
  # 内核：featheragent serve --root . --codec json
  python tools/feather_client.py --root . --codec json
  python tools/feather_client.py --root . --codec json --text "你好"

Windows 可直接读 <root>\\runtime\\ipc\\endpoint 得到命名管道名；
也可用 --pipe '\\\\.\\pipe\\featheragent-xxxxxxxxxxxxxxxx' 指定。

协议（v1.1.2）:
  4 字节小端无符号帧长 + JSON/MessagePack 体，最大 64 MiB。
  默认内核 codec=msgpack；本脚本默认 json，需服务端 --codec json。
"""
import argparse
import json
import os
import queue
import socket
import struct
import sys
import threading
import time


def endpoint_from_root(root):
    p = os.path.join(root, "runtime", "ipc", "endpoint")
    if os.path.isfile(p):
        return open(p, "r", encoding="utf-8").read().strip()
    p = os.path.join(root, "runtime", "ipc", "featheragent.sock")
    return p


def open_transport(endpoint):
    if endpoint.startswith("\\\\.\\pipe\\") or endpoint.startswith("//./pipe/"):
        return open(endpoint, "r+b", buffering=0)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(endpoint)
    return s


class Client:
    def __init__(self, endpoint, codec="json"):
        self.endpoint = endpoint
        self.codec = codec
        self.f = open_transport(endpoint)
        self.q = queue.Queue()
        self.events = queue.Queue()
        self.next_id = 1
        self.closed = False
        threading.Thread(target=self._reader, daemon=True).start()
        if codec == "msgpack":
            import msgpack  # optional
            self.msgpack = msgpack

    def _reader(self):
        try:
            while not self.closed:
                hdr = self._read_exact(4)
                if not hdr:
                    break
                (n,) = struct.unpack("<I", hdr)
                if n > 64 * 1024 * 1024:
                    raise RuntimeError("frame too large")
                body = self._read_exact(n)
                if body is None:
                    break
                if self.codec == "json":
                    msg = json.loads(body.decode("utf-8"))
                else:
                    msg = self.msgpack.unpackb(body, raw=False)
                self.q.put(msg)
        except Exception as e:
            self.q.put({"__error__": str(e)})

    def _read_exact(self, n):
        out = b""
        while len(out) < n:
            try:
                b = self.f.recv(n - len(out)) if isinstance(self.f, socket.socket) else self.f.read(n - len(out))
            except Exception:
                return None
            if not b:
                return None
            out += b
        return out

    def send(self, obj):
        if self.codec == "json":
            body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        else:
            body = self.msgpack.packb(obj, use_bin_type=True)
        data = struct.pack("<I", len(body)) + body
        if isinstance(self.f, socket.socket):
            self.f.sendall(data)
        else:
            self.f.write(data)
            self.f.flush()

    def call(self, method, params=None, timeout=10):
        rid = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}})
        deadline = time.time() + timeout
        while True:
            left = deadline - time.time()
            if left <= 0:
                raise TimeoutError(f"{method} timeout")
            try:
                msg = self.q.get(timeout=left)
            except queue.Empty:
                raise TimeoutError(f"{method} timeout")
            if "__error__" in msg:
                raise RuntimeError(msg["__error__"])
            if msg.get("method") == "event":
                self.events.put(msg)
                continue
            if msg.get("id") == rid:
                if "error" in msg:
                    raise RuntimeError(json.dumps(msg["error"], ensure_ascii=False))
                return msg.get("result")

    def next_event(self, timeout):
        try:
            return self.events.get_nowait()
        except queue.Empty:
            pass
        try:
            return self.q.get(timeout=timeout)
        except queue.Empty:
            return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--pipe", default=None, help="Windows 命名管道名")
    ap.add_argument("--codec", choices=["json", "msgpack"], default="json")
    ap.add_argument("--session", default="cli")
    ap.add_argument("--text", default=None)
    ap.add_argument("--timeout", type=float, default=180)
    a = ap.parse_args()

    endpoint = a.pipe or endpoint_from_root(a.root)
    print(f"[connect] {endpoint}", flush=True)
    c = Client(endpoint, a.codec)
    info = c.call("initialize", {})
    print(json.dumps(info, ensure_ascii=False), flush=True)
    if not a.text:
        print(json.dumps(c.call("system.info", {}), ensure_ascii=False), flush=True)
        return 0

    c.call("session.create", {"id": a.session})
    c.call("event.subscribe", {"kinds": []})
    c.call("chat.send", {"session": a.session, "text": a.text})
    deadline = time.time() + a.timeout
    while time.time() < deadline:
        ev = c.next_event(min(1.0, max(0.0, deadline - time.time())))
        if ev is None:
            continue
        print(json.dumps(ev, ensure_ascii=False), flush=True)
        p = ev.get("params", {})
        if p.get("kind") == "chat.finished":
            return 0
    print("[timeout] no chat.finished", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
