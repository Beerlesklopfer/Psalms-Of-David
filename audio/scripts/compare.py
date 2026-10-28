# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Compares a recording of the null sink with the decoded source (see verify.sh).
import struct, sys
import numpy as np

def read_wav_f32(path):
    d = open(path, 'rb').read()
    pos = 12
    fmt = None
    while pos < len(d):
        cid = d[pos:pos+4]; size = struct.unpack('<I', d[pos+4:pos+8])[0]
        if cid == b'fmt ':
            fmt = struct.unpack('<HHIIHH', d[pos+8:pos+24])
        elif cid == b'data':
            size = min(size, len(d) - pos - 8)
            data = np.frombuffer(d[pos+8:pos+8+size], dtype='<f4')
            return fmt, data
        pos += 8 + size + (size & 1)
    raise SystemExit('no data chunk')

fmt, rec = read_wav_f32(sys.argv[1])
tag, ch, rate = fmt[0], fmt[1], fmt[2]
rec = rec.reshape(-1, ch)
ref = np.fromfile(sys.argv[2], dtype='<f4').reshape(-1, 2)   # 24 kHz stereo reference
ref_rate = 24000
print(f'recording: format={tag} {ch} ch {rate} Hz, {len(rec)/rate:.2f} s')
print(f'reference: {len(ref)/ref_rate:.2f} s at {ref_rate} Hz')

# Reference to the recording rate (linear interpolation is enough for an envelope comparison)
t_ref = np.arange(len(ref)) / ref_rate
t_new = np.arange(int(len(ref) * rate / ref_rate)) / rate
ref_r = np.stack([np.interp(t_new, t_ref, ref[:, c]) for c in range(2)], axis=1)

def active(x, thr=1e-4):
    idx = np.where(np.abs(x).max(axis=1) > thr)[0]
    return (idx[0], idx[-1]) if len(idx) else (0, 0)

a0, a1 = active(rec)
b0, b1 = active(ref_r)
print(f'sound in recording: {a0/rate:.2f} s .. {a1/rate:.2f} s (length {(a1-a0)/rate:.2f} s)')
print(f'sound in reference: {b0/rate:.2f} s .. {b1/rate:.2f} s (length {(b1-b0)/rate:.2f} s)')

n = min(a1 - a0, b1 - b0)
x = rec[a0:a0+n]; y = ref_r[b0:b0+n]
rms = lambda v: float(np.sqrt(np.mean(v**2)))
print(f'RMS recording {rms(x):.4f}  reference {rms(y):.4f}  ratio {rms(x)/rms(y):.3f}')

# Envelope correlation in 50 ms blocks, tolerant to a small offset
blk = int(rate * 0.05)
def env(v):
    m = len(v) // blk
    return np.sqrt((v[:m*blk].reshape(m, blk, -1) ** 2).mean(axis=(1, 2)))
ex, ey = env(x), env(y)
best = max(((np.corrcoef(ex[max(0, s):len(ex)+min(0, s)], ey[max(0, -s):len(ey)-max(0, s)])[0, 1], s) for s in range(-6, 7)), key=lambda t: t[0])
print(f'envelope correlation {best[0]:.4f} at offset {best[1]*50} ms')

# Sample-level check: correlate the left channel directly after a coarse alignment
lag_max = int(rate * 0.05)
xs = x[:, 0][: rate * 2]; ys = y[:, 0][: rate * 2]
cc = np.correlate(xs, ys[lag_max:-lag_max], mode='valid')
lag = int(np.argmax(cc)) - lag_max
print(f'sample-level lag {lag} samples ({lag/rate*1000:.2f} ms)')
ok = best[0] > 0.98 and 0.95 < rms(x)/rms(y) < 1.05 and abs((a1-a0)-(b1-b0)) < rate * 0.1
print('RESULT:', 'OK - the recording matches the decoded file' if ok else 'MISMATCH')
sys.exit(0 if ok else 1)
