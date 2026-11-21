# Security notes and known weaknesses

Tracked weaknesses (accepted for now, revisit with the console / hardening work).

| ID | Area | Weakness | Status |
| --- | --- | --- | --- |
| W1 | AirPlay receiver | shairport-sync (AirPlay 2 mode) accepts any LAN client, no password; TCP 7000, UDP 319/320 open on all interfaces | accepted (2026-11-19) |
| W2 | Faust GUI | faust-gui httpd (TCP 5510) and OSC (UDP 5510-5512) have no auth, bound to 0.0.0.0 | open |
| W3 | Host firewall | no host firewall; host has a global IPv6 address, relies on router to block inbound | open |
| W4 | Access recovery | single SSH key authorized for user lab; root and lab passwords locked; lab has NOPASSWD sudo and desktop autologin | open |
| W5 | Trillium Console (pilabd) | TCP 8410 on all interfaces (IPv4 and IPv6), plain HTTP: the pairing token crosses the LAN unencrypted; `/api/v1/info` and `/api/v1/endpoints` answer without a token (product, version, device id, host name) | open (2026-11-20) |

## Changes

- 2026-11-19: rpcbind.service and rpcbind.socket disabled and masked (TCP/UDP 111 closed; NFS not used).
  Undo: `sudo systemctl unmask rpcbind.service rpcbind.socket && sudo systemctl enable --now rpcbind.socket`

## Trillium Console (pilabd, port 8410)

- Service: `pilabd` (user unit), listening on `[::]:8410` (IPv4 and IPv6, all interfaces).
  Home LAN only; nothing is forwarded from the internet. With no host firewall (W3), the
  router is what keeps the global IPv6 address unreachable from outside.
- Auth: a 48-hex-character pairing token in `~/.config/pilab/console-token` (mode 0600),
  checked in constant time. It is printed only by `pilabctl pair` in a terminal on the Pi,
  never logged, never in mDNS, never in the web page. Browsers keep it in local storage.
  Rotate with `pilabctl pair --rotate`.
- Unauthenticated: `GET /api/v1/info`, `GET /api/v1/endpoints`, and the static web files.
- What a token holder can do: change sounds, routing, scenes, master level (capped at
  −6 dBFS), start/stop the synth, record locally, link/unlink audio and MIDI ports,
  change settings. Not possible from the console: shell access, switching AirPlay, VNC,
  SSH or other services, reading the NAS credentials file.
- Planned: TLS with a self-signed device certificate pinned by `id`, and a QR pairing flow.

## Access model (2026-11-19)

- Users: root (password locked, no SSH keys), lab (password locked, key-only SSH, passwordless sudo).
- sshd: PasswordAuthentication no, KbdInteractive no, PermitRootLogin without-password.
- Fallbacks: local keyboard + monitor (desktop autologins as lab), or add a key to
  /home/lab/.ssh/authorized_keys by mounting the boot media on another machine.
