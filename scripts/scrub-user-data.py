#!/usr/bin/env python3
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Replace SSH public keys in a cloud-init user-data file with a placeholder."""
import re
import sys
from pathlib import Path

PLACEHOLDER = "<YOUR_SSH_PUBKEY>"
KEY_LINE = re.compile(
    r"^([ \t]*-[ \t]+)(?:['\"]?)(?:"
    r"ssh-(?:ed25519|rsa|dss)|"
    r"ecdsa-sha2-nistp\d+|"
    r"sk-ssh-ed25519@openssh\.com|"
    r"sk-ecdsa-sha2-nistp256@openssh\.com"
    r")[ \t]+\S+.*$",
    re.MULTILINE,
)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: scrub-user-data.py path/to/user-data", file=sys.stderr)
        return 2
    path = Path(sys.argv[1])
    text = path.read_text()
    new, n = KEY_LINE.subn(r"\1" + PLACEHOLDER, text)
    path.write_text(new)
    print(f"scrubbed {n} SSH key line(s) in {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
