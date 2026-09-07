#!/usr/bin/env python3
"""Release the page cache held by large streamed model files.

An H3 evaluation streams the whole denoiser trunk from disk on every step —
about 69 GB of h2d traffic per run on the 3090 Ti box. Those pages stay resident
in the page cache afterwards, so a few consecutive renders leave the host with
almost no free memory (measured: 52 GB of 62 GB in buff/cache, 3 GB free). The
runtime guard samples PSI and cancels the job when full-memory stalls cross its
threshold, which killed three H3 renders on 2026-09-06 with the child itself
peaking under 5 GB — the pressure was the cache, not the job.

posix_fadvise(POSIX_FADV_DONTNEED) drops those clean pages without root and
without touching the files. It is advisory: dirty pages are not discarded, and
anything still mapped by a live process stays. Re-reading is a cold read again,
which is exactly what a streamed runtime does anyway.

Usage: release_streamed_pages.py [--min-mib N] PATH [PATH ...]
Paths may be files or directories; directories are walked. Never fails the
caller — a page-cache hint is not worth aborting a finished render over.
"""
import os
import sys

SUFFIXES = (".safetensors", ".difbind", ".diftensor", ".gguf", ".bin", ".pt")


def release(path, min_bytes):
    """Return (files, bytes) advised away under `path`."""
    files = 0
    total = 0
    if os.path.isfile(path):
        candidates = [path]
    else:
        candidates = []
        for dirpath, _dirnames, names in os.walk(path):
            for name in names:
                candidates.append(os.path.join(dirpath, name))
    for candidate in candidates:
        if not candidate.endswith(SUFFIXES):
            continue
        try:
            size = os.path.getsize(candidate)
            if size < min_bytes:
                continue
            fd = os.open(candidate, os.O_RDONLY)
        except OSError:
            continue
        try:
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
            files += 1
            total += size
        except OSError:
            pass
        finally:
            os.close(fd)
    return files, total


def main(argv):
    min_mib = 64
    paths = []
    i = 0
    while i < len(argv):
        if argv[i] == "--min-mib" and i + 1 < len(argv):
            try:
                min_mib = int(argv[i + 1])
            except ValueError:
                pass
            i += 2
            continue
        paths.append(argv[i])
        i += 1
    if not paths:
        return 0
    files = 0
    total = 0
    for path in paths:
        if not path:
            continue
        try:
            got_files, got_bytes = release(path, min_mib * 1024 * 1024)
        except Exception:
            continue
        files += got_files
        total += got_bytes
    if files:
        sys.stderr.write(
            "[page-cache] released %d streamed file(s), %.1f GB advised\n"
            % (files, total / 1e9)
        )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Exception:
        sys.exit(0)
