# Vulnerability scan: trillium-base-0.4.0-beta.1-unit-debian (2026-11-25 UTC, grype database of that day)

Unique (source package, CVE) pairs after de-duplication: 1806 (kernel: 334, everything else: 1472).

## Everything except the kernel

| Severity | Fix in the archive | No fix yet | Will not fix |
| --- | ---: | ---: | ---: |
| Critical | 1 | 21 | 24 |
| High | 19 | 84 | 252 |
| Medium | 25 | 102 | 352 |
| Low | 2 | 23 | 77 |
| Negligible | 0 | 441 | 18 |
| Unknown | 6 | 4 | 21 |

## Kernel (leads only)

| Severity | Fix in the archive | No fix yet | Will not fix |
| --- | ---: | ---: | ---: |
| Critical | 0 | 12 | 0 |
| High | 0 | 59 | 0 |
| Medium | 0 | 51 | 1 |
| Low | 0 | 1 | 0 |
| Negligible | 0 | 58 | 0 |
| Unknown | 0 | 152 | 0 |

## Known-exploited (CISA KEV) matches

- openssh CVE-2026-123158 (Negligible, not-fixed)

## Critical and High with a fix already in the archive (upgrade clears these)

| Source package | Installed | Fixed in | CVEs |
| --- | --- | --- | ---: |
| gpgme1.0 | 1.24.2-3 | 1.24.2-3+deb13u1 | 1 |
| krb5 | 1.21.3-5+deb13u1 | 1.21.3-5+deb13u2 | 6 |
| rsync | 3.5.0+ds1-0+deb13u1 | 3.5.0+ds1-0+deb13u2 | 1 |
| ruby3.3 | 3.3.13-0+deb13u1 | 3.3.14-0+deb13u1 | 1 |
| sudo | 1.9.16p2-3+deb13u2 | 1.9.16p2-3+deb13u5 | 1 |
| webkit2gtk | 2.54.0-1~deb13u3 | 2.54.3-1~deb13u1 | 10 |

## Critical and High by source package (no fix or will not fix), top 15

| Source package | Critical+High |
| --- | ---: |
| ffmpeg | 57 |
| pillow | 39 |
| curl | 31 |
| xwayland | 20 |
| libsoup2.4 | 19 |
| libsoup3 | 19 |
| busybox | 16 |
| pyjwt | 11 |
| hplip | 10 |
| openssl | 9 |
| libxml2 | 8 |
| gst-plugins-good1.0 | 7 |
| openssh | 6 |
| python3.13 | 5 |
| bluez | 4 |
