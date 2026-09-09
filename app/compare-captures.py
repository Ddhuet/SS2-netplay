"""Bounded report of two private capture folders; never prints save contents."""
import argparse
import re
from pathlib import Path


def read_capture(path):
    hashes, local, remote = {}, {}, {}
    for line in (path / "events.txt").open():
        if match := re.search(r"COMPONENT (\d+) (\S+) (\d+) (\w+)", line):
            hashes[int(match[1]), match[2]] = match[4]
        if match := re.search(r"ADVANCE (\d+) (\d+)", line):
            tick = int(match[1])
            assert tick not in local, f"duplicate advance {tick}"
            local[tick] = int(match[2])
        if match := re.search(r"Input \{ tick: (\d+), keys: (\d+), advantage:", line):
            tick = int(match[1])
            assert tick not in remote, f"duplicate remote input {tick}"
            remote[tick] = int(match[2])
    return hashes, local, remote


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host", type=Path)
    parser.add_argument("guest", type=Path)
    args = parser.parse_args()
    captures = [args.host, args.guest]
    data = [read_capture(p) for p in captures]
    for seat in range(2):
        local, remote = data[seat][1], data[1 - seat][2]
        common = local.keys() & remote.keys()
        mismatch = [t for t in sorted(common) if local[t] != remote[t]]
        missing = len(set(range(max(remote) + 1)) - remote.keys())
        print(f"Seat {seat}: {len(common)} input pairs, {len(mismatch)} mismatches, {missing} remote gaps")
    common = data[0][0].keys() & data[1][0].keys()
    mismatch = [k for k in sorted(common) if data[0][0][k] != data[1][0][k]]
    print(f"Compared {len(common)} component hashes; {len(mismatch)} mismatches")
    for tick, name in mismatch[:20]:
        print(f"  boundary={tick} component={name}")
    directories = {p.name for p in captures[0].iterdir() if p.is_dir()} & {p.name for p in captures[1].iterdir() if p.is_dir()}
    for label in sorted(directories):
        indices = []
        for cap in captures:
            rows = (cap / label / "index.txt").read_text().splitlines()
            indices.append({row.split()[1]: row.split()[0] for row in rows})
        assert indices[0].keys() == indices[1].keys(), "component inventories differ"
        count = 0
        for name in indices[0]:
            a, b = [(cap / label / index[name]).read_bytes() for cap, index in zip(captures, indices)]
            if a != b:
                count += 1
                offsets = [i for i, (x, y) in enumerate(zip(a, b)) if x != y]
                print(f"  {label} {name}: lengths={len(a)}/{len(b)}, differing shared bytes={len(offsets)}, first offsets={[hex(i) for i in offsets[:24]]}")
        print(f"{label}: {count} differing components")


if __name__ == "__main__":
    main()
