# Security notes and known weaknesses

Tracked weaknesses (accepted for now, revisit with the console / hardening work).

| ID | Area | Weakness | Status |
| --- | --- | --- | --- |
| W1 | AirPlay receiver | shairport-sync (AirPlay 2 mode) accepts any LAN client, no password; TCP 7000, UDP 319/320 open on all interfaces | accepted (2026-11-19) |
| W2 | Faust GUI | faust-gui httpd (TCP 5510) and OSC (UDP 5510-5512) have no auth, bound to 0.0.0.0 | open |
| W3 | Host firewall | no host firewall; host has a global IPv6 address, relies on router to block inbound | open |
| W4 | Access recovery | single SSH key authorized for user lab; root and lab passwords locked; lab has NOPASSWD sudo and desktop autologin | open |

## Changes

- 2026-11-19: rpcbind.service and rpcbind.socket disabled and masked (TCP/UDP 111 closed; NFS not used).
  Undo: `sudo systemctl unmask rpcbind.service rpcbind.socket && sudo systemctl enable --now rpcbind.socket`

## Access model (2026-11-19)

- Users: root (password locked, no SSH keys), lab (password locked, key-only SSH, passwordless sudo).
- sshd: PasswordAuthentication no, KbdInteractive no, PermitRootLogin without-password.
- Fallbacks: local keyboard + monitor (desktop autologins as lab), or add a key to
  /home/lab/.ssh/authorized_keys by mounting the boot media on another machine.
