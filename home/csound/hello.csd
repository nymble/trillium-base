<CsoundSynthesizer>
<CsOptions>
-odac -+rtaudio=jack -b 64 -B 256 -d -m0
</CsOptions>
<CsInstruments>
; Copyright (C) 2026 Nymble
; SPDX-License-Identifier: GPL-3.0-or-later
sr = 48000
ksmps = 32
nchnls = 2
0dbfs = 1
instr 1
  kenv linseg 0, 0.05, 1, p3-0.3, 0.8, 0.25, 0
  a1 vco2 0.08*kenv, p4
  af moogladder a1, 800 + 1500*kenv, 0.4
  outs af, af
endin
</CsInstruments>
<CsScore>
i1 0 2.5 220
i1 0 2.5 277.18
i1 0 2.5 329.63
</CsScore>
</CsoundSynthesizer>
