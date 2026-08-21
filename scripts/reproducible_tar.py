#!/usr/bin/env python3
"""Create and compare canonical .tar.gz release archives using only stdlib."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import os
from pathlib import Path
import tarfile


def canonical_info(path: Path, arcname: str, epoch: int) -> tarfile.TarInfo:
    info = tarfile.TarInfo(arcname)
    info.mtime = epoch
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    if path.is_symlink():
        info.type = tarfile.SYMTYPE
        info.mode = 0o777
        info.linkname = os.readlink(path)
    elif path.is_dir():
        info.type = tarfile.DIRTYPE
        info.mode = 0o755
    else:
        info.type = tarfile.REGTYPE
        info.mode = 0o755 if "/bin/" in arcname else 0o644
        info.size = path.stat().st_size
    return info


def create(source: Path, output: Path, epoch: int) -> None:
    source = source.resolve()
    root = source.name
    paths = [source, *sorted(source.rglob("*"), key=lambda path: path.relative_to(source).as_posix())]
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=epoch, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.GNU_FORMAT) as archive:
                for path in paths:
                    relative = path.relative_to(source).as_posix()
                    arcname = root if relative == "." else f"{root}/{relative}"
                    info = canonical_info(path, arcname, epoch)
                    if info.isreg():
                        with path.open("rb") as contents:
                            archive.addfile(info, contents)
                    else:
                        archive.addfile(info)


def manifest(archive_path: Path) -> list[tuple[object, ...]]:
    result = []
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive.getmembers():
            digest = ""
            if member.isfile():
                extracted = archive.extractfile(member)
                if extracted is None:
                    raise RuntimeError(f"could not read {member.name}")
                digest = hashlib.sha256(extracted.read()).hexdigest()
            result.append(
                (
                    member.name,
                    member.type,
                    member.mode,
                    member.uid,
                    member.gid,
                    member.mtime,
                    member.linkname,
                    member.size,
                    digest,
                )
            )
    return result


def compare(left: Path, right: Path) -> None:
    if left.read_bytes() != right.read_bytes():
        raise SystemExit("archive bytes differ")
    left_manifest = manifest(left)
    right_manifest = manifest(right)
    if left_manifest != right_manifest:
        raise SystemExit("archive member metadata or contents differ")
    for name, _kind, mode, uid, gid, _mtime, _link, _size, _digest in left_manifest:
        if uid != 0 or gid != 0:
            raise SystemExit(f"non-canonical owner for {name}: {uid}:{gid}")
        expected_mode = 0o755 if "/bin/" in name or name.endswith("/bin") else 0o644
        if name.endswith("/") or _kind == tarfile.DIRTYPE:
            expected_mode = 0o755
        if _kind == tarfile.SYMTYPE:
            expected_mode = 0o777
        if mode != expected_mode:
            raise SystemExit(f"non-canonical mode for {name}: {mode:o}")


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    create_parser = subparsers.add_parser("create")
    create_parser.add_argument("source", type=Path)
    create_parser.add_argument("output", type=Path)
    create_parser.add_argument("--epoch", required=True, type=int)
    compare_parser = subparsers.add_parser("compare")
    compare_parser.add_argument("left", type=Path)
    compare_parser.add_argument("right", type=Path)
    args = parser.parse_args()
    if args.command == "create":
        create(args.source, args.output, args.epoch)
    else:
        compare(args.left, args.right)


if __name__ == "__main__":
    main()
