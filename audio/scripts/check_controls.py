# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Checks the recording of the null sink against the decoded file and the position
# log of the engine (see verify-controls.sh).
import struct
import sys

import numpy as np

RATE = 48000


def read_wav_f32(path):
    d = open(path, 'rb').read()
    pos = 12
    while pos < len(d):
        cid = d[pos:pos + 4]
        size = struct.unpack('<I', d[pos + 4:pos + 8])[0]
        if cid == b'fmt ':
            ch = struct.unpack('<HHIIHH', d[pos + 8:pos + 24])[1]
        elif cid == b'data':
            size = min(size, len(d) - pos - 8)
            return ch, np.frombuffer(d[pos + 8:pos + 8 + size], dtype='<f4')
        pos += 8 + size + (size & 1)
    raise SystemExit('no data chunk')


def load(rec_path, ref_path):
    ch, rec = read_wav_f32(rec_path)
    rec = rec.reshape(-1, ch)[:, 0]                       # left channel
    ref = np.fromfile(ref_path, dtype='<f4').reshape(-1, 2)[:, 0]
    # the reference is at 24 kHz: bring it to the recording rate
    t_ref = np.arange(len(ref)) / 24000
    t_new = np.arange(int(len(ref) * RATE / 24000)) / RATE
    return rec, np.interp(t_new, t_ref, ref)


def parse_log(path):
    pos, events = [], []
    for line in open(path):
        p = line.split()
        if p[0] == 'POS':
            pos.append((int(p[1]), int(p[2]), p[3], float(p[4]), int(p[5])))
        elif p[0] == 'DO':
            events.append((int(p[1]), p[2]))
    return pos, events


def at(pos, elapsed):
    """The log line closest to `elapsed` (ms)."""
    return min(pos, key=lambda r: abs(r[0] - elapsed))


def active(x, thr=1e-4):
    return np.abs(x) > thr


def first_sound(x, start=0):
    idx = np.where(active(x[start:]))[0]
    return None if len(idx) == 0 else (start + idx[0]) / RATE


def silent_gap(x, after_s, min_len_s=0.4):
    """Start and end (s) of the first stretch of digital silence of `min_len_s` or more after `after_s`."""
    a = active(x)
    i = int(after_s * RATE)
    n = int(min_len_s * RATE)
    # smooth: a sample is "silent" if nothing above the threshold within 5 ms
    win = int(0.005 * RATE)
    quiet = np.convolve(a.astype(np.int32), np.ones(win, dtype=np.int32), 'same') == 0
    run = 0
    for k in range(i, len(x)):
        run = run + 1 if quiet[k] else 0
        if run >= n:
            start = k - run + 1
            end = start
            while end < len(x) and quiet[end]:
                end += 1
            return start / RATE, end / RATE
    return None


def rms(x):
    return float(np.sqrt(np.mean(x ** 2))) if len(x) else 0.0


def locate(seg, ref, around_s, radius_s):
    """Where in `ref` does `seg` sit? Searches around `around_s` +- `radius_s`; returns seconds and Pearson correlation."""
    lo = max(0, int((around_s - radius_s) * RATE))
    hi = min(len(ref), int((around_s + radius_s) * RATE) + len(seg))
    window = ref[lo:hi]
    if len(window) < len(seg):
        return None, 0.0
    n = 1 << (len(window) + len(seg)).bit_length()
    corr = np.fft.irfft(np.fft.rfft(window, n) * np.conj(np.fft.rfft(seg, n)), n)[:len(window) - len(seg) + 1]
    k = int(np.argmax(corr))
    cand = window[k:k + len(seg)]
    r = float(np.corrcoef(cand, seg)[0, 1]) if np.std(cand) > 0 and np.std(seg) > 0 else 0.0
    return (lo + k) / RATE, r


def check(cond, text, failures):
    print(('  ok   ' if cond else '  FAIL ') + text)
    if not cond:
        failures.append(text)


def scenario_pause(rec, ref, pos, events, f):
    t0 = first_sound(rec)
    print(f'sound starts at {t0:.2f} s in the recording')
    # before the pause: the file from its start, in step with the recording
    seg = rec[int((t0 + 0.1) * RATE):int((t0 + 2.0) * RATE)]
    start, r = locate(seg, ref, 0.1, 0.3)
    check(r > 0.99 and abs(start - 0.1) < 0.1, f'before the pause the recording is the file from its start (at {start:.2f} s, r={r:.4f})', f)

    gap = silent_gap(rec, t0 + 1.0)
    check(gap is not None, 'a stretch of silence follows (the pause)', f)
    if gap is None:
        return
    g0, g1 = gap
    print(f'silence from {g0:.2f} s to {g1:.2f} s ({g1 - g0:.2f} s)')
    check(2.6 <= g1 - g0 <= 3.6, f'the pause lasts about the 3 s it was asked for ({g1 - g0:.2f} s)', f)

    p_pause = at(pos, 3300)[1]
    p_held = at(pos, 5700)[1]
    check(abs(p_held - p_pause) <= 20, f'the position stands still while paused ({p_pause} ms, then {p_held} ms)', f)
    check(at(pos, 4500)[2] == 'Paused', 'the state is Paused in between', f)

    # after the pause: the file goes on where it stopped, not from the start and not further ahead
    seg = rec[int((g1 + 0.15) * RATE):int((g1 + 2.0) * RATE)]
    start, r = locate(seg, ref, p_pause / 1000 + 0.15, 0.6)
    check(r > 0.99, f'after the pause the recording is the file again (r={r:.4f})', f)
    check(abs(start - (p_pause / 1000 + 0.15)) < 0.25, f'it goes on where it stopped: at {start:.2f} s, stopped at {p_pause / 1000:.2f} s', f)
    check(at(pos, 8500)[2] == 'Playing' and at(pos, 8500)[1] > p_pause + 1500, 'and the position moves on again', f)


def scenario_seek(rec, ref, pos, events, f):
    t0 = first_sound(rec)
    seg = rec[int((t0 + 0.1) * RATE):int((t0 + 1.8) * RATE)]
    start, r = locate(seg, ref, 0.1, 0.3)
    check(r > 0.99 and abs(start - 0.1) < 0.1, f'before the seek the recording is the file from its start (r={r:.4f})', f)

    gap = silent_gap(rec, t0 + 1.5, min_len_s=0.02)
    jump = (gap[1] - gap[0]) if gap else 0.0
    print(f'break around the seek: {jump * 1000:.0f} ms')
    check(jump < 0.7, f'the break at the seek is short ({jump * 1000:.0f} ms)', f)

    after_from = (gap[1] if gap else t0 + 2.2) + 0.3
    seg = rec[int(after_from * RATE):int((after_from + 2.0) * RATE)]
    expected = 8.0 + 0.3
    start, r = locate(seg, ref, expected, 1.0)
    check(r > 0.99, f'after the seek the recording is the file again (r={r:.4f})', f)
    check(abs(start - expected) < 0.35, f'from the place asked for: at {start:.2f} s, expected about {expected:.2f} s', f)
    p = at(pos, 4000)
    check(p[2] == 'Playing' and 9000 <= p[1] <= 10500, f'the position follows the jump ({p[1]} ms at 4 s, {p[2]})', f)
    check(all(row[2] != 'Failed' for row in pos), 'no failure', f)


def scenario_volume(rec, ref, pos, events, f):
    t0 = first_sound(rec)
    def ratio(a, b):
        seg = rec[int((t0 + a) * RATE):int((t0 + b) * RATE)]
        base = ref[int(a * RATE):int(b * RATE)]
        return rms(seg) / rms(base)
    full = ratio(1.5, 3.4)
    half = ratio(4.8, 6.8)
    print(f'level relative to the file: {full:.3f} before, {half:.3f} after')
    check(0.9 < full < 1.15, f'at first the level equals the file ({full:.3f})', f)
    check(0.40 < half / full < 0.60, f'after volume=0.5 the level is half ({half / full:.3f} of before)', f)
    check(at(pos, 6000)[3] == 0.5 and at(pos, 1000)[3] == 1.0, 'the engine reports the volume', f)
    # the shape is untouched, only the level changes
    seg = rec[int((t0 + 4.8) * RATE):int((t0 + 6.8) * RATE)]
    start, r = locate(seg, ref, 4.8, 0.3)
    # At half the level the quantisation noise weighs twice as much; 0.98 still means the same shape.
    check(r > 0.98, f'and the sound is still the same sound (r={r:.4f})', f)


def scenario_paused_start(rec, ref, pos, events, f):
    t0 = first_sound(rec)
    print(f'sound starts at {t0:.2f} s in the recording (recorder started about 1.5 s before the player)')
    check(t0 >= 3.9, f'nothing is heard while the player starts paused ({t0:.2f} s)', f)
    check(t0 <= 5.3, f'it starts when asked to ({t0:.2f} s)', f)
    check(at(pos, 1500)[2] == 'Paused' and at(pos, 1500)[1] <= 30, 'the state is Paused and the position stays at 0', f)
    seg = rec[int((t0 + 0.1) * RATE):int((t0 + 2.0) * RATE)]
    start, r = locate(seg, ref, 0.1, 0.3)
    check(r > 0.99 and abs(start - 0.1) < 0.1, f'and it plays the file from its start (at {start:.2f} s, r={r:.4f})', f)


SCENARIOS = {'pause': scenario_pause, 'seek': scenario_seek, 'volume': scenario_volume, 'paused-start': scenario_paused_start}

if __name__ == '__main__':
    name, rec_path, ref_path, log_path = sys.argv[1:5]
    rec, ref = load(rec_path, ref_path)
    pos, events = parse_log(log_path)
    failures = []
    print(f'scenario {name}: recording {len(rec) / RATE:.2f} s, {len(pos)} position lines')
    SCENARIOS[name](rec, ref, pos, events, failures)
    print('RESULT:', 'OK' if not failures else f'{len(failures)} check(s) FAILED')
    sys.exit(0 if not failures else 1)
