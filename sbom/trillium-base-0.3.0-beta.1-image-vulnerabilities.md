# Vulnerability scan: trillium-base-0.3.0-beta.1-image (2026-11-25 UTC, grype database of that day)

Unique (source package, CVE) pairs after de-duplication: 796 (kernel: 334, everything else: 462).

## Everything except the kernel

| Severity | Fix in the archive | No fix yet | Will not fix |
| --- | ---: | ---: | ---: |
| Critical | 0 | 3 | 11 |
| High | 6 | 29 | 69 |
| Medium | 6 | 15 | 109 |
| Low | 0 | 3 | 18 |
| Negligible | 0 | 176 | 3 |
| Unknown | 6 | 4 | 4 |

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
| krb5 | 1.21.3-5+deb13u1 | 1.21.3-5+deb13u2 | 6 |

## Critical and High by source package (no fix or will not fix), top 15

| Source package | Critical+High |
| --- | ---: |
| curl | 31 |
| pyjwt | 11 |
| openssl | 9 |
| libxml2 | 8 |
| openssh | 6 |
| python3.13 | 5 |
| bluez | 4 |
| expat | 4 |
| util-linux | 4 |
| dracut | 3 |
| python-urllib3 | 3 |
| wget | 3 |
| sqlite3 | 3 |
| perl | 2 |
| libsndfile | 2 |
