#!/usr/bin/env python3
"""Compare the level of WAV files octave by octave, against the first.

Prints one row per file: its level in each octave, in dB, relative to the
first file's. Use it to see whether a render is darker or brighter than its DI
or a reference render, e.g.:

    python3 -I octave-balance.py di.wav reference-render.wav render.wav

Each file's level is its average power over the whole file (Welch's method),
so compare files of about the same length: a long silent tail lowers every
octave of one file by the same amount. Stereo is averaged to mono, and the
files may differ in sample rate and bit depth.
"""

import argparse
import math
import pathlib
import warnings

import numpy as np
import scipy.io.wavfile
import scipy.signal


def read_mono(path):
    """The file's sample rate and its samples as mono floats in -1..1."""
    with warnings.catch_warnings():
        # Reaper and other hosts write chunks scipy doesn't know; the audio is still read.
        warnings.simplefilter("ignore", scipy.io.wavfile.WavFileWarning)
        rate, samples = scipy.io.wavfile.read(path)
    if np.issubdtype(samples.dtype, np.integer):
        samples = samples / float(np.iinfo(samples.dtype).max + 1)
    samples = samples.astype(np.float64)
    if samples.ndim > 1:
        samples = samples.mean(axis=1)
    return rate, samples


def octave_levels(path, edges):
    """The file's power in each octave between consecutive `edges`, in dB."""
    rate, samples = read_mono(path)
    frequencies, density = scipy.signal.welch(samples, rate, nperseg=min(16384, len(samples)))
    width = frequencies[1] - frequencies[0]
    levels = []
    for low, high in zip(edges, edges[1:]):
        band = (frequencies >= low) & (frequencies < high)
        power = density[band].sum() * width if high <= rate / 2 else math.nan
        levels.append(10 * math.log10(power) if power > 0 else -math.inf)
    return levels


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("reference", type=pathlib.Path, help="the file the others are compared with")
    parser.add_argument("files", type=pathlib.Path, nargs="+")
    parser.add_argument(
        "--lowest-hz", type=float, default=31.25, help="where the first octave starts (default 31.25)"
    )
    parser.add_argument("--octaves", type=int, default=9, help="how many octaves (default 9)")
    args = parser.parse_args()

    edges = [args.lowest_hz * 2**octave for octave in range(args.octaves + 1)]
    reference = octave_levels(args.reference, edges)
    width = max(len(path.name) for path in [args.reference, *args.files])

    print(f"{'octave from (Hz)':<{width}}", *(f"{low:>7.0f}" for low in edges[:-1]))
    print(f"{args.reference.name:<{width}}", *(f"{level:>7.1f}" for level in reference), " dB")
    for path in args.files:
        relative = [level - base for level, base in zip(octave_levels(path, edges), reference)]
        print(f"{path.name:<{width}}", *(f"{level:>+7.1f}" for level in relative))


if __name__ == "__main__":
    main()
