# Security: deployment notes and defaults

Trillium Base is meant for a trusted LAN (a home or studio network behind a router, no
ports forwarded from the internet). These notes describe what a default build exposes,
the risks worth knowing, and how to harden a unit. They are guidance, not a guarantee.

## Exposure in a default build

| Area | Default | Risk | Hardening |
| --- | --- | --- | --- |
| Trillium Console (`pilabd`) | TCP 8410 on all interfaces (IPv4 and IPv6), plain HTTP, pairing token required for everything except `/api/v1/info`, `/api/v1/endpoints` and the static web files | The token crosses the LAN unencrypted; `info` reveals product, version, device id and host name | Keep it on a trusted LAN; rotate the token after sharing it; TLS with a pinned device certificate is planned |
| AirPlay receiver (optional, off by default) | shairport-sync AirPlay 2, TCP 7000 and UDP 319/320 on all interfaces | Any LAN client can play through the DAC | Set a shairport-sync `password` or access list, or leave it off |
| Faust web controls (optional) | faust-gui httpd TCP 5510, OSC UDP 5510-5512, no authentication, all interfaces | Anyone on the LAN can change parameters | Run only when needed; bind to localhost and tunnel over SSH |
| SuperCollider server (optional) | `scsynth` UDP 57110 bound to 127.0.0.1 | Local only | Keep the localhost bind |
| Desktop (VNC, optional) | wayvnc bound to 127.0.0.1:5900 | Local only; reach it through an SSH tunnel | Keep the localhost bind |
| Host firewall | none configured | With a global IPv6 address, only the router blocks inbound traffic | Add nftables rules allowing SSH, 8410 and mDNS from the LAN only |
| Unused RPC | rpcbind is not needed (no NFS) | TCP/UDP 111 open if left enabled | `sudo systemctl disable --now rpcbind.service rpcbind.socket && sudo systemctl mask rpcbind.service rpcbind.socket` |

## Trillium Console (pilabd, port 8410)

- `pilabd` runs as a user service, listening on `[::]:8410`. Do not forward it from the
  internet; use a VPN such as Tailscale for remote access if you need it.
- Auth: a 48-hex-character pairing token in `~/.config/pilab/console-token` (mode 0600),
  checked in constant time. It is printed only by `trillium-base pair` (`pilabctl pair`)
  in a terminal on the device: never logged, never in mDNS, never in the web page.
  Browsers keep it in local storage. Rotate with `trillium-base pair --rotate`.
- What a token holder can do: change instruments, routing, scenes and the master level
  (capped at −6 dBFS); start and stop the synth; record locally; link and unlink audio and
  MIDI ports; change settings. Not possible from the console: shell access, reading
  credential files. Service switches for SSH and other system services need passwordless
  sudo for the service user and report it when that is missing.
- Audio safety defaults: the synth starts at −18 dBFS at most, fades in, and has an
  output limiter, because the DAC drives headphones.
- Planned: TLS with a self-signed device certificate pinned by device `id`, and a QR
  pairing flow that never puts the token itself in the QR code.

## Access model guidance

- Key-only SSH: `PasswordAuthentication no`, `KbdInteractiveAuthentication no`, root login
  disabled or key-only, passwords locked for root and the service user.
- Authorize at least two SSH keys (or keep a second recovery path) so one lost key does
  not lock you out. Recovery without SSH: local keyboard and monitor, or add a key to
  `~/.ssh/authorized_keys` by mounting the boot media on another machine.
- Passwordless sudo and desktop autologin are convenient on a lab unit. Turn them off on
  a unit that leaves a trusted space.
- Credentials for optional services (for example a NAS account for recordings) belong in
  root-only files that the console never reads; the console shows only the file path.
- Never commit secrets, private keys, SSH host keys or disk images to this repository.
