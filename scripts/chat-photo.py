#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Photos that a person sent into a Claude Code chat, sent on to David.

A model sees a photo of the chat as a picture, not as its bytes: it cannot write the photo down as base64. Claude Code
keeps the photo in the transcript of the session (~/.claude/projects/<project>/<session>.jsonl), so this script takes
it from there and sends it to an upload address of the MCP tool photo_upload_link. The bytes never pass through the model.

  chat-photo.py list                       the photos of the newest session: number, time, type, size
  chat-photo.py send URL [N ...]           sends photos N (default: the last one) to URL, in that order
  chat-photo.py save DIR [N ...]           writes them to DIR (to crop them first, then send with curl)

--session FILE takes another transcript than the newest one of all projects.
"""
import argparse
import base64
import glob
import json
import os
import sys
import urllib.request


def newest_transcript():
    files = glob.glob(os.path.expanduser("~/.claude/projects/*/*.jsonl"))
    if not files:
        sys.exit("no Claude Code transcript found")
    return max(files, key=os.path.getmtime)


def photos(transcript):
    """The pictures the person sent, in the order they came: (time, media type, bytes)."""
    out = []
    with open(transcript, encoding="utf-8") as f:
        for line in f:
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            if entry.get("type") != "user":
                continue
            content = (entry.get("message") or {}).get("content")
            if not isinstance(content, list):
                continue
            for block in content:
                source = block.get("source") if isinstance(block, dict) and block.get("type") == "image" else None
                if source and source.get("type") == "base64":
                    out.append((entry.get("timestamp", ""), source.get("media_type", ""), base64.b64decode(source["data"])))
    return out


def pick(found, numbers):
    if not found:
        sys.exit("this session has no photos")
    numbers = numbers or [len(found)]
    for n in numbers:
        if not 1 <= n <= len(found):
            sys.exit(f"no photo {n}; there are {len(found)}")
    return [(n, found[n - 1]) for n in numbers]


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--session", help="transcript file (default: the newest)")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list")
    s = sub.add_parser("send")
    s.add_argument("url")
    s.add_argument("n", nargs="*", type=int)
    d = sub.add_parser("save")
    d.add_argument("dir")
    d.add_argument("n", nargs="*", type=int)
    a = p.parse_args()
    transcript = a.session or newest_transcript()
    found = photos(transcript)
    if a.cmd == "list":
        print(transcript)
        for i, (when, mime, data) in enumerate(found, 1):
            print(f"{i}\t{when}\t{mime}\t{len(data)} bytes")
        return
    for n, (_, mime, data) in pick(found, a.n):
        if a.cmd == "save":
            os.makedirs(a.dir, exist_ok=True)
            path = os.path.join(a.dir, f"photo-{n:02}.{'png' if mime == 'image/png' else 'jpg'}")
            with open(path, "wb") as f:
                f.write(data)
            print(path)
        else:
            req = urllib.request.Request(a.url, data=data, method="POST", headers={"Content-Type": mime or "application/octet-stream"})
            with urllib.request.urlopen(req, timeout=60) as r:
                answer = json.load(r)
            print(f"photo {n} -> David photo {answer.get('photo')}")


if __name__ == "__main__":
    main()
