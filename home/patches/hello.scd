// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
// First patch: short FM chord through the DAC (expects scsynth already running on UDP 57110)
s = Server.remote(\lab, NetAddr("127.0.0.1", 57110));
s.doWhenBooted {
    fork {
        s.initTree; s.sync;
        "connected, playing".postln;
        SynthDef(\fm, { |freq = 220, amp = 0.12, dur = 4|
            var env = EnvGen.kr(Env.linen(0.3, dur - 1.3, 1.0), doneAction: 2);
            var mod = SinOsc.ar(freq * 2.01) * freq * 1.5 * env;
            var sig = SinOsc.ar(freq + mod) * env * amp;
            Out.ar(0, Pan2.ar(sig, LFNoise1.kr(0.5)));
        }).add;
        s.sync;
        [220, 277.18, 329.63, 440].do { |f| Synth(\fm, [\freq, f]) };
        5.wait;
        ("peak CPU %: " ++ s.peakCPU.round(0.1)).postln;
        6.wait; 0.exit;
    };
};
