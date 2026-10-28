# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Checks the speed control (see verify-speed.sh): the position log must advance at the
# set speed, and the recording must have the same pitch as the file (a faster tape
# would shift the spectrum, a time stretcher does not).
import sys

import numpy as np

sys.path.insert(0, __file__.rsplit('/', 1)[0])
from check_controls import load, parse_log  # noqa: E402


def centroid(x, rate):
    n = 4096
    frames = [x[i:i + n] * np.hanning(n) for i in range(0, len(x) - n, n // 2)]
    spec = np.zeros(n // 2 + 1)
    for f in frames:
        if np.sqrt(np.mean(f * f)) > 0.01:
            spec += np.abs(np.fft.rfft(f)) ** 2
    freqs = np.fft.rfftfreq(n, 1 / rate)
    return float((spec * freqs).sum() / spec.sum())


rec_path, ref_path, log_path, speed = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
rec, ref = load(rec_path, ref_path)
pos, _ = parse_log(log_path)
# pos: rows of (elapsed ms, position ms, ...); take a stretch well after the change
def slope(lo, hi):
    rows = [p for p in pos if lo <= p[0] <= hi]
    return (rows[-1][1] - rows[0][1]) / (rows[-1][0] - rows[0][0])


# The null sink delivers in large blocks, so the speed is compared with the speed
# before the change instead of with 1.0.
ratio = slope(5000, 9000) / slope(500, 4000)
print(f'position advances {ratio:.3f}x faster than before (set {speed})')
ok = abs(ratio - speed) < 0.05

heard_to = max(p[1] for p in pos) / 1000
ref = ref[int(1.0 * 48000): int(heard_to * 48000)]
c_ref = centroid(ref, 48000)
c_rec = centroid(rec[int(48000 * 2.5):], 48000)
print(f'spectral centroid: file {c_ref:.0f} Hz, recording {c_rec:.0f} Hz ({c_rec / c_ref:.2f})')
ok = ok and 0.8 < c_rec / c_ref < 1.25
print('PASS' if ok else 'FAIL')
sys.exit(0 if ok else 1)
