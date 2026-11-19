#!/bin/sh
# Re-sync live files from the Pi into this repo. Read-only on the Pi.
# Usage (from anywhere):
#   scripts/pull-from-pi.sh
# Override the SSH target with PILAB_HOST (default: lab@pilab.local).
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
HOST="${PILAB_HOST:-lab@pilab.local}"
export LC_ALL=C

ssh -o BatchMode=yes -o ConnectTimeout=8 "$HOST" 'true' >/dev/null

copy() {
    src=$1
    dest=$2
    mkdir -p "$(dirname "$dest")"
    scp -o BatchMode=yes -q "$HOST:$src" "$dest"
}

copy /boot/firmware/config.txt            "$ROOT/boot/config.txt"
copy /boot/firmware/user-data             "$ROOT/boot/user-data"
copy /boot/firmware/issue.txt             "$ROOT/boot/issue.txt" || true

copy /etc/security/limits.d/95-audio.conf                 "$ROOT/etc/security/limits.d/95-audio.conf"
copy /etc/systemd/system/cpu-performance.service          "$ROOT/etc/systemd/system/cpu-performance.service"
copy /etc/systemd/system.conf.d/10-cpu-affinity.conf      "$ROOT/etc/systemd/system.conf.d/10-cpu-affinity.conf"
copy /etc/pipewire/pipewire.conf.d/10-lowlatency.conf     "$ROOT/etc/pipewire/pipewire.conf.d/10-lowlatency.conf"
copy /etc/wayvnc/config                                   "$ROOT/etc/wayvnc/config"
copy /etc/systemd/system/getty@tty1.service.d/autologin.conf \
    "$ROOT/etc/systemd/system/getty@tty1.service.d/autologin.conf" || true

copy /home/lab/.config/wayvnc/config                      "$ROOT/home/.config/wayvnc/config"
copy /home/lab/.config/systemd/user/wayvnc-lab.service    "$ROOT/home/.config/systemd/user/wayvnc-lab.service"

mkdir -p "$ROOT/home/patches"
scp -o BatchMode=yes -q "$HOST:/home/lab/patches/*.scd" "$ROOT/home/patches/" || true

copy /usr/local/bin/sc-play        "$ROOT/bin/sc-play"
copy /usr/local/bin/pilab-bench    "$ROOT/bin/pilab-bench"
chmod 755 "$ROOT/bin/sc-play" "$ROOT/bin/pilab-bench"

if [ -f "$HOME/bin/pilab-desktop.command" ]; then
    mkdir -p "$ROOT/mac"
    cp "$HOME/bin/pilab-desktop.command" "$ROOT/mac/pilab-desktop.command"
    chmod 755 "$ROOT/mac/pilab-desktop.command"
fi

# Never copy wayvnc TLS keys or anything under ~/.ssh
mkdir -p "$ROOT/packages"
ssh -o BatchMode=yes "$HOST" 'uname -a' > "$ROOT/packages/uname.txt"
ssh -o BatchMode=yes "$HOST" 'apt-mark showmanual' > "$ROOT/packages/apt-mark-showmanual.txt"
ssh -o BatchMode=yes "$HOST" "dpkg-query -W -f='\${Package}\t\${Version}\n' linux-image-rpi-v8-rt linux-headers-rpi-v8-rt pipewire pipewire-bin pipewire-jack pipewire-pulse wireplumber rtkit alsa-utils supercollider supercollider-server supercollider-language sc3-plugins puredata csound faust rpd-wayland-core rpd-preferences wayvnc labwc jackd2 2>/dev/null" \
    > "$ROOT/packages/key-packages.txt" || true

mkdir -p "$ROOT/bench"
# Drop previous copies so removed bench files do not linger, then refresh.
find "$ROOT/bench" -type f -name '*.txt' -delete
scp -o BatchMode=yes -q "$HOST:/var/lib/pilab/bench/*" "$ROOT/bench/" 2>/dev/null || true

# Replace any SSH public keys in cloud-init user-data with a placeholder.
python3 "$ROOT/scripts/scrub-user-data.py" "$ROOT/boot/user-data"

echo "Updated $ROOT from $HOST"
