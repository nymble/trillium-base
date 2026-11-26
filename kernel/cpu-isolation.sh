#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
#
# Trillium Base CPU isolation: reserve one core (default CPU 3) for the audio thread.
#
#   cpu-isolation.sh status          show what is applied (cmdline, IRQs, workqueues)
#   cpu-isolation.sh apply [--dry-run]
#                                    back up cmdline.txt, add the isolation parameters the
#                                    running kernel supports, install the boot-time service.
#                                    Takes effect after a reboot. Idempotent.
#   cpu-isolation.sh runtime         steer IRQs, unbound workqueues and movable kthreads off
#                                    the audio core (run at boot by the service; safe to rerun)
#   cpu-isolation.sh revert          restore cmdline.txt without the parameters, disable the
#                                    service, give every core back to IRQs and workqueues.
#                                    Takes effect fully after a reboot.
#
# Environment: AUDIO_CPU (default 3), HOUSEKEEPING (default 0-2),
#              CMDLINE (default /boot/firmware/cmdline.txt).
#
# Kernel support: isolcpus and irqaffinity work on every kernel. nohz_full needs
# CONFIG_NO_HZ_FULL and rcu_nocbs needs CONFIG_RCU_NOCB_CPU; the stock Raspberry Pi RT
# kernel has neither, so they are only added when the running kernel's config has them
# (the Trillium kernel built from kernel/trillium-rt.fragment does).
set -eu

AUDIO_CPU=${AUDIO_CPU:-3}
HOUSEKEEPING=${HOUSEKEEPING:-0-2}
CMDLINE=${CMDLINE:-/boot/firmware/cmdline.txt}
BACKUP_DIR=${BACKUP_DIR:-/var/lib/trillium/cmdline-backups}
SERVICE=trillium-cpu-isolation.service
SELF_INSTALLED=/usr/local/sbin/trillium-cpu-isolation
KEYS='isolcpus nohz_full rcu_nocbs irqaffinity'

die() { echo "cpu-isolation: $*" >&2; exit 1; }
need_root() { [ "$(id -u)" = 0 ] || die "run as root"; }

kconfig() {
    for f in /boot/config-"$(uname -r)" /proc/config.gz; do
        [ -r "$f" ] || continue
        case $f in *.gz) zcat "$f" ;; *) cat "$f" ;; esac
        return 0
    done
}
has_kopt() { kconfig 2>/dev/null | grep -qx "CONFIG_$1=y"; }

# Housekeeping CPU list -> hex mask (for workqueue cpumask files).
hk_mask() {
    python3 - "$HOUSEKEEPING" <<'PY'
import sys
m = 0
for part in sys.argv[1].split(","):
    a, _, b = part.partition("-")
    for c in range(int(a), int(b or a) + 1):
        m |= 1 << c
print(format(m, "x"))
PY
}

wanted_params() {
    p="isolcpus=managed_irq,domain,$AUDIO_CPU irqaffinity=$HOUSEKEEPING"
    has_kopt NO_HZ_FULL && p="$p nohz_full=$AUDIO_CPU"
    has_kopt RCU_NOCB_CPU && p="$p rcu_nocbs=$AUDIO_CPU"
    echo "$p"
}

# cmdline.txt without any of our keys, as a single line.
stripped_cmdline() {
    tr -s ' \n' '\n\n' < "$CMDLINE" | awk -v keys="$KEYS" '
        BEGIN { n = split(keys, k, " "); for (i = 1; i <= n; i++) drop[k[i]] = 1 }
        NF { split($0, kv, "="); if (!(kv[1] in drop)) printf "%s%s", (c++ ? " " : ""), $0 }
        END { print "" }'
}

backup_cmdline() {
    mkdir -p "$BACKUP_DIR"
    stamp=$(date +%Y%m%d-%H%M%S)
    cp -p "$CMDLINE" "$BACKUP_DIR/cmdline.txt.$stamp"
    # First-ever backup, kept on the boot partition too so it can be restored from
    # another computer by mounting the card (no flashing needed).
    [ -e "$CMDLINE.pre-isolation" ] || cp -p "$CMDLINE" "$CMDLINE.pre-isolation"
    echo "backup: $BACKUP_DIR/cmdline.txt.$stamp"
}

write_cmdline() {
    tmp=$(mktemp "$(dirname "$CMDLINE")/.cmdline.XXXXXX")
    printf '%s\n' "$1" > "$tmp"
    sync "$tmp" 2>/dev/null || sync
    mv "$tmp" "$CMDLINE"
    sync
}

runtime() {
    need_root
    moved=0 fixed=0
    for d in /proc/irq/[0-9]*; do
        [ -w "$d/smp_affinity_list" ] || continue
        if echo "$HOUSEKEEPING" > "$d/smp_affinity_list" 2>/dev/null; then
            moved=$((moved + 1))
        else
            fixed=$((fixed + 1))   # per-CPU or managed IRQs refuse; that is expected
        fi
    done
    echo "$HOUSEKEEPING" > /proc/irq/default_smp_affinity 2>/dev/null ||
        printf '%s\n' "$(hk_mask)" > /proc/irq/default_smp_affinity 2>/dev/null || true
    mask=$(hk_mask)
    for f in /sys/devices/virtual/workqueue/cpumask /sys/bus/workqueue/devices/*/cpumask; do
        [ -w "$f" ] && echo "$mask" > "$f" 2>/dev/null || true
    done
    # Unbound kernel threads (rcu_preempt, kcompactd, ...) that are allowed on every
    # core: pin them to housekeeping. Per-CPU threads (*/3) refuse and stay.
    kt=0
    for p in $(ps -eo pid=,ppid= | awk '$2 == 2 || $1 == 2 { print $1 }'); do
        cur=$(taskset -pc "$p" 2>/dev/null | sed 's/.*: //') || continue
        case $cur in *"$AUDIO_CPU"*) taskset -pc "$HOUSEKEEPING" "$p" >/dev/null 2>&1 && kt=$((kt + 1)) ;; esac
    done
    echo "runtime: $moved IRQs -> $HOUSEKEEPING ($fixed fixed/per-CPU), workqueues -> 0x$mask, $kt kthreads moved"
}

status() {
    echo "kernel:      $(uname -r)"
    echo "cmdline.txt: $(grep -oE '(isolcpus|nohz_full|rcu_nocbs|irqaffinity)=[^ ]*' "$CMDLINE" | tr '\n' ' ')"
    echo "booted with: $(tr ' ' '\n' < /proc/cmdline | grep -E '^(isolcpus|nohz_full|rcu_nocbs|irqaffinity)=' | tr '\n' ' ')"
    echo "wanted:      $(wanted_params)"
    echo "isolated:    $(cat /sys/devices/system/cpu/isolated)"
    echo "nohz_full:   $(cat /sys/devices/system/cpu/nohz_full 2>/dev/null || echo 'n/a (kernel without NO_HZ_FULL)')"
    n=0 t=0
    for f in /proc/irq/[0-9]*/smp_affinity_list; do
        t=$((t + 1))
        python3 -c "import sys; s=set()
for p in sys.argv[1].split(','):
    a,_,b=p.partition('-'); s.update(range(int(a),int(b or a)+1))
sys.exit(0 if int(sys.argv[2]) in s else 1)" "$(cat "$f")" "$AUDIO_CPU" && n=$((n + 1))
    done
    echo "IRQs allowed on CPU $AUDIO_CPU: $n/$t"
    echo "workqueue cpumask: $(cat /sys/devices/virtual/workqueue/cpumask 2>/dev/null)"
    echo "service:     $(systemctl is-enabled $SERVICE 2>/dev/null || true)"
}

install_service() {
    install -D -m 755 "$0" "$SELF_INSTALLED"
    cat > /etc/systemd/system/$SERVICE <<UNIT
# Installed by kernel/cpu-isolation.sh (Trillium Base)
[Unit]
Description=Trillium Base: keep IRQs and kernel work off the audio core (CPU $AUDIO_CPU)
After=systemd-modules-load.service systemd-udev-trigger.service

[Service]
Type=oneshot
Environment=AUDIO_CPU=$AUDIO_CPU HOUSEKEEPING=$HOUSEKEEPING
ExecStart=$SELF_INSTALLED runtime
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
UNIT
    systemctl daemon-reload
    systemctl enable $SERVICE >/dev/null 2>&1
}

apply() {
    need_root
    [ -r "$CMDLINE" ] || die "$CMDLINE not found"
    [ "$(grep -c . "$CMDLINE")" = 1 ] || die "$CMDLINE must be exactly one line; fix it by hand first"
    new="$(stripped_cmdline) $(wanted_params)"
    cur=$(tr -s ' ' < "$CMDLINE" | sed 's/ *$//')
    echo "current: $cur"
    echo "new:     $new"
    if [ "${1:-}" = --dry-run ]; then echo "(dry run, nothing written)"; return 0; fi
    if [ "$cur" = "$new" ]; then
        echo "cmdline.txt already up to date"
    else
        backup_cmdline
        write_cmdline "$new"
        echo "cmdline.txt updated; reboot to apply"
    fi
    install_service
    echo "service $SERVICE installed and enabled"
}

revert() {
    need_root
    backup_cmdline
    write_cmdline "$(stripped_cmdline)"
    systemctl disable --now $SERVICE >/dev/null 2>&1 || true
    rm -f /etc/systemd/system/$SERVICE
    systemctl daemon-reload
    for d in /proc/irq/[0-9]*; do echo "0-3" > "$d/smp_affinity_list" 2>/dev/null || true; done
    for f in /sys/devices/virtual/workqueue/cpumask /sys/bus/workqueue/devices/*/cpumask; do
        [ -w "$f" ] && echo f > "$f" 2>/dev/null || true
    done
    echo "reverted: $(cat "$CMDLINE")"
    echo "reboot to drop isolcpus/irqaffinity from the running kernel"
}

case ${1:-status} in
    status) status ;;
    apply) shift; apply "$@" ;;
    runtime) runtime ;;
    revert) revert ;;
    *) sed -n '5,20p' "$0"; exit 2 ;;
esac
