<Cabbage>
form caption("Pi Lab Cabbage Demo") size(460, 230), colour(28, 32, 40), pluginId("plcd")
label bounds(10, 8, 440, 18), text("Saw -> Moog ladder filter. Tick Gate to play."), fontColour(200, 200, 200)
rslider bounds(10, 35, 100, 100), channel("freq"), range(40, 880, 110, 0.5, 0.01), text("Freq"), textColour(220, 220, 220)
rslider bounds(120, 35, 100, 100), channel("cutoff"), range(80, 8000, 1200, 0.4, 1), text("Cutoff"), textColour(220, 220, 220)
rslider bounds(230, 35, 100, 100), channel("res"), range(0, 0.9, 0.3, 1, 0.01), text("Resonance"), textColour(220, 220, 220)
checkbox bounds(350, 65, 90, 30), channel("gate"), text("Gate"), value(0), colour:1(120, 220, 120)
hslider bounds(10, 150, 440, 50), channel("vol"), range(0, 0.1, 0.03, 1, 0.001), text("Volume"), textColour(220, 220, 220)
</Cabbage>
<CsoundSynthesizer>
<CsOptions>
-n -d -m0d
</CsOptions>
<CsInstruments>
; Copyright (C) 2026 M-of-N
; SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
; Pi Lab Cabbage demo: always-on voice, silent until the Gate box is ticked.
; Volume is capped at 0.1 to keep test levels quiet.
sr = 48000
ksmps = 32
nchnls = 2
0dbfs = 1

instr 1
  kfreq  chnget "freq"
  kcut   chnget "cutoff"
  kres   chnget "res"
  kvol   chnget "vol"
  kgate  chnget "gate"
  kfreq  portk kfreq, 0.02
  kcut   portk kcut, 0.02
  kenv   portk kgate, 0.05
  kvol   limit kvol, 0, 0.1
  asaw   vco2 0.8, kfreq
  asaw2  vco2 0.8, kfreq * 1.005
  afilt  moogladder (asaw + asaw2) * 0.5, limit(kcut, 20, 12000), kres
  aout   = afilt * a(kvol * kenv)
  outs aout, aout
endin
</CsInstruments>
<CsScore>
i1 0 z
</CsScore>
</CsoundSynthesizer>
