#!/bin/sh
# Copyright (C) 2026 Nymble
# SPDX-License-Identifier: GPL-3.0-or-later
# Build and install the Cabbage 2 IDE (JUCE 6.1.2) on the Pi against Debian's Csound 6.18.
# Run on the Pi as user lab:  sh build-cabbage.sh /path/to/cabbage-linux-aarch64.patch
# Takes about 13 minutes on a Pi 5 with -j3 and about 850 MB in ~/src/cabbage2.
set -eu
PATCH="${1:-$(dirname "$0")/../patches/cabbage-linux-aarch64.patch}"
PATCH="$(cd "$(dirname "$PATCH")" && pwd)/$(basename "$PATCH")"
CABBAGE_REF=v2-master-final          # = fe44ff9194373c883b60f118688ead06b519714d (Cabbage 2.10.7)
JUCE_REF=6.1.2                       # the JUCE version Cabbage 2's build.py pins
SRC="$HOME/src/cabbage2"

# 1. Build dependencies (Csound 6 headers incl. csound.hpp, JUCE Linux deps, Ninja).
sudo apt-get -o DPkg::Lock::Timeout=600 install -y -q \
  git cmake ninja-build pkg-config python3-requests \
  libcsound64-dev libcsnd-dev libsndfile1-dev \
  libfreetype-dev libx11-dev libxinerama-dev libxrandr-dev libxcursor-dev libxcomposite-dev \
  libxext-dev mesa-common-dev libgl-dev libasound2-dev libjack-jackd2-dev \
  libcurl4-openssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev grim

# 2. Sources. build.py is not used: it switches to the develop branch, downloads the VST SDK,
#    CabbageRack (x86_64 only) and the manual, and builds every plugin target.
mkdir -p "$HOME/src"
[ -d "$SRC/.git" ] || git clone --depth 1 --branch "$CABBAGE_REF" https://github.com/rorywalsh/cabbage.git "$SRC"
cd "$SRC"
if [ ! -d JUCE/modules ]; then
  rmdir JUCE 2>/dev/null || true
  git clone -q --depth 1 --branch "$JUCE_REF" https://github.com/juce-framework/JUCE
  # Cabbage's own JUCE patches, same order as build.py.
  for p in juce_mac_Windowing_snapshot StandaloneWrapper AUWrapper UtilityWrapper VST2Wrapper VST3Wrapper LinuxFile; do
    git apply "patches/$p.patch"
  done
  # juceaide needs C++17 with GCC 14 (build.py does the same edit).
  sed -i 's/^\(\s*\)juce_add_console_app(juceaide)\s*$/&\n\1target_compile_features(juceaide PRIVATE cxx_std_17)/' \
    JUCE/extras/Build/juceaide/CMakeLists.txt
fi

# 3. Pi Lab patch (CMakeLists.txt + one source file):
#    - skip juce_set_vst2_sdk_path when the VST2 SDK is absent, and turn off VST2 hosting
#    - JUCE_JACK=1 so the IDE can use JACK (PipeWire) instead of ALSA
#    - CABBAGE_AUTOPLAY=1 env var: start the instrument opened from the command line
git apply --check "$PATCH" 2>/dev/null && git apply "$PATCH" || echo "patch already applied"

# 4. Configure and build only the IDE target (the CabbagePlugin* targets need the VST SDK).
mkdir -p build && cd build
cmake -G Ninja -DCMAKE_BUILD_TYPE=Release -DPROJECT_NAME=Cabbage \
  -DJucePlugin_Manufacturer=CabbageAudio -DJucePlugin_ManufacturerCode=Cabb \
  -DJucePlugin_Desc=CabbagePlugin -DCabbagePro=0 -DBluetooth=0 -DCustomStandalone=0 ..
ninja -j3

# 5. Install. Examples and themes go where the Linux build looks for them (hard-coded /usr/share).
cd "$SRC"
sudo install -D -m 755 build/Cabbage_artefacts/Release/Cabbage /opt/cabbage/bin/Cabbage
sudo install -d /usr/share/doc/cabbage /usr/share/cabbage
sudo cp -r Examples /usr/share/doc/cabbage/
sudo cp -r Themes /usr/share/cabbage/
sudo install -D -m 644 Images/cabbage.png /usr/share/icons/hicolor/512x512/apps/cabbage.png
echo "Installed /opt/cabbage/bin/Cabbage. Install bin/cabbage from the repo as /usr/local/bin/cabbage."
