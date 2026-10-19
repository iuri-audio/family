#!/usr/bin/env python3
"""Align detector onset sequences and summarize their attack-start offsets."""

import argparse
import math
import pathlib
import re
import statistics
import subprocess


ROW_PATTERN = re.compile(r"\s*([0-9]+(?:\.[0-9]+)?) s \(sample (\d+)\), late (.+)$")


def read_onsets(detector, detector_args, path):
    output = subprocess.check_output([str(detector), *detector_args, str(path)], text=True)
    events = []
    for line in output.splitlines():
        match = ROW_PATTERN.fullmatch(line)
        if not match:
            continue
        seconds = float(match.group(1))
        sample = int(match.group(2))
        late_text = match.group(3)
        late_ms = None if "no rising edge found" in late_text else float(late_text.split(" ms")[0])
        start = None if late_ms is None else seconds - late_ms / 1000.0
        events.append({"sample": sample, "time": seconds, "start": start})
    return events


def align(left, right, gap_cost_ms):
    rows = len(left) + 1
    columns = len(right) + 1
    costs = [[math.inf] * columns for _ in range(rows)]
    choices = [[None] * columns for _ in range(rows)]
    costs[0][0] = 0.0

    for i in range(rows):
        for j in range(columns):
            base = costs[i][j]
            if not math.isfinite(base):
                continue
            if i < len(left) and j < len(right):
                left_time = left[i]["start"] if left[i]["start"] is not None else left[i]["time"]
                right_time = right[j]["start"] if right[j]["start"] is not None else right[j]["time"]
                cost = base + abs(right_time - left_time) * 1000.0
                if cost < costs[i + 1][j + 1]:
                    costs[i + 1][j + 1] = cost
                    choices[i + 1][j + 1] = (i, j, "match")
            if i < len(left) and base + gap_cost_ms < costs[i + 1][j]:
                costs[i + 1][j] = base + gap_cost_ms
                choices[i + 1][j] = (i, j, "left-only")
            if j < len(right) and base + gap_cost_ms < costs[i][j + 1]:
                costs[i][j + 1] = base + gap_cost_ms
                choices[i][j + 1] = (i, j, "right-only")

    matches = []
    left_only = 0
    right_only = 0
    i, j = len(left), len(right)
    while i or j:
        choice = choices[i][j]
        if choice is None:
            raise RuntimeError("failed to backtrack onset alignment")
        previous_i, previous_j, operation = choice
        if operation == "match":
            matches.append((previous_i, previous_j))
        elif operation == "left-only":
            left_only += 1
        else:
            right_only += 1
        i, j = previous_i, previous_j
    matches.reverse()
    return matches, left_only, right_only


def percentile(values, fraction):
    ordered = sorted(values)
    position = fraction * (len(ordered) - 1)
    below = math.floor(position)
    above = min(below + 1, len(ordered) - 1)
    return ordered[below] + (ordered[above] - ordered[below]) * (position - below)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir", type=pathlib.Path)
    parser.add_argument("--detector", type=pathlib.Path, required=True)
    parser.add_argument("--detector-arg", action="append", default=[])
    parser.add_argument("--render-suffix", required=True)
    parser.add_argument(
        "--reference-pair",
        action="append",
        nargs=3,
        default=[],
        metavar=("LABEL", "LEFT", "RIGHT"),
    )
    parser.add_argument("--reported-samples", type=float, default=0)
    parser.add_argument("--sample-rate", type=int, default=44100)
    parser.add_argument("--gap-cost-ms", type=float, default=30)
    args = parser.parse_args()

    run_dir = args.run_dir.resolve()
    detector = args.detector.resolve()
    if not detector.is_file():
        raise SystemExit(f"onset detector not found: {detector}")

    pairs = []
    input_dir = run_dir / "input"
    render_dir = run_dir / "renders"
    for left in sorted(input_dir.glob("*.wav")):
        right = render_dir / f"{left.stem}{args.render_suffix}.wav"
        if not right.is_file():
            raise SystemExit(f"render is missing: {right}")
        pairs.append((left.name, left, right))

    for label, left_text, right_text in args.reference_pair:
        left = pathlib.Path(left_text)
        right = pathlib.Path(right_text)
        if not left.is_file() or not right.is_file():
            raise SystemExit(f"reference pair is missing: {left} / {right}")
        pairs.append((label, left, right))
    if not pairs:
        raise SystemExit(f"no left/right pairs found under {input_dir}")

    summary = []
    onset_pairs = []
    all_offsets_ms = []
    for label, left_path, right_path in pairs:
        left_events = read_onsets(detector, args.detector_arg, left_path)
        right_events = read_onsets(detector, args.detector_arg, right_path)
        matches, left_only, right_only = align(left_events, right_events, args.gap_cost_ms)
        offsets_ms = []
        for left_index, right_index in matches:
            left = left_events[left_index]
            right = right_events[right_index]
            if left["start"] is None or right["start"] is None:
                continue
            offset_ms = (right["start"] - left["start"]) * 1000.0
            offsets_ms.append(offset_ms)
            all_offsets_ms.append(offset_ms)
            onset_pairs.append((label, left_index + 1, left["time"], right["time"], offset_ms))

        if offsets_ms:
            summary.append(
                (
                    label,
                    len(left_events),
                    len(right_events),
                    len(offsets_ms),
                    statistics.median(offsets_ms),
                    percentile(offsets_ms, 0.1),
                    percentile(offsets_ms, 0.9),
                    left_only,
                    right_only,
                )
            )
        else:
            summary.append((label, len(left_events), len(right_events), 0, None, None, None, left_only, right_only))

    if not all_offsets_ms:
        raise SystemExit("no paired attack-start estimates")

    median_ms = statistics.median(all_offsets_ms)
    p10_ms = percentile(all_offsets_ms, 0.1)
    p90_ms = percentile(all_offsets_ms, 0.9)
    reported_ms = args.reported_samples * 1000.0 / args.sample_rate
    total_ms = median_ms + reported_ms
    total_samples = total_ms * args.sample_rate / 1000.0

    with (run_dir / "onset-summary.tsv").open("w", encoding="ascii") as output:
        output.write(
            "pair\tleft_onsets\tright_onsets\tpaired_starts\tmedian_ms\tp10_ms\tp90_ms\tleft_unmatched\tright_unmatched\n"
        )
        for row in summary:
            output.write("\t".join("" if value is None else str(value) for value in row) + "\n")

    with (run_dir / "onset-pairs.tsv").open("w", encoding="ascii") as output:
        output.write("pair\tattack\tleft_onset_s\tright_onset_s\tresidual_ms\tresidual_samples\n")
        for label, attack, left_time, right_time, offset_ms in onset_pairs:
            output.write(
                f"{label}\t{attack}\t{left_time:.6f}\t{right_time:.6f}\t{offset_ms:.3f}\t"
                f"{offset_ms * args.sample_rate / 1000.0:.1f}\n"
            )

    print(f"pairs: {len(pairs)}")
    print(f"paired attack starts: {len(all_offsets_ms)}")
    print("residual offset median/p10/p90 (ms):", *(round(value, 3) for value in (median_ms, p10_ms, p90_ms)))
    print(
        f"estimated median total latency: {total_ms:.3f} ms ({total_samples:.1f} samples at {args.sample_rate} Hz)"
    )
    print(f"pair summary: {run_dir / 'onset-summary.tsv'}")
    print(f"paired onsets: {run_dir / 'onset-pairs.tsv'}")


if __name__ == "__main__":
    main()