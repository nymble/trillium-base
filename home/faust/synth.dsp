import("stdfaust.lib");

freq      = hslider("[0] freq[style:knob][unit:Hz]", 220, 40, 2000, 0.1);
cutoff    = hslider("[1] cutoff[style:knob][unit:Hz]", 1200, 80, 8000, 1);
res       = hslider("[2] resonance[style:knob]", 0.4, 0, 0.95, 0.01);
lfo_rate  = hslider("[3] LFO rate[style:knob][unit:Hz]", 0.5, 0.01, 20, 0.01);
lfo_depth = hslider("[4] LFO depth[style:knob][unit:Hz]", 200, 0, 2000, 1);
vol       = hslider("[5] volume[style:knob]", 0.1, 0, 0.3, 0.001);
gate      = checkbox("[6] gate");

lfo   = os.osc(lfo_rate) * lfo_depth;
voice = os.sawtooth(freq) : ve.moog_vcf(res, max(80, cutoff + lfo)) * vol * gate;
process = hgroup("synth", voice) <: _,_;
