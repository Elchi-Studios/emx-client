#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Writes the notices of the crates a binary is built from: each crate's
name, version and licence, and the licence and notice files it ships.

    scripts/third-party-notices.py emx-cli > cli/THIRD-PARTY-NOTICES.txt
    scripts/third-party-notices.py emx-desktop > desktop/THIRD-PARTY-NOTICES.txt

Only what is linked into the binary counts: build scripts, procedural
macros and test dependencies are left out. Needs cargo, nothing else."""

import json
import os
import subprocess
import sys

NOTICE_FILES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")


def main(root_name):
    meta = json.loads(
        subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"])
    )
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == root_name)
    seen, todo = set(), [root]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            kinds = {k["kind"] for k in dep["dep_kinds"]}
            target = packages[dep["pkg"]]
            macro = any("proc-macro" in t["kind"] for t in target["targets"])
            if None in kinds and not macro:
                todo.append(dep["pkg"])
    ours = {p["id"] for p in meta["packages"] if p["source"] is None}
    out = sys.stdout
    out.write(f"Third-party notices for {root_name}\n\n")
    out.write(f"{root_name} is built from the crates below, under the licences named.\n")
    for pid in sorted(seen - ours, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        p = packages[pid]
        out.write("\n" + "=" * 72 + "\n")
        out.write(f"{p['name']} {p['version']}\n")
        out.write(f"Licence: {p.get('license') or 'see the files below'}\n")
        if p.get("repository"):
            out.write(f"Source: {p['repository']}\n")
        folder = os.path.dirname(p["manifest_path"])
        for name in sorted(os.listdir(folder)):
            if name.upper().startswith(NOTICE_FILES) and os.path.isfile(os.path.join(folder, name)):
                with open(os.path.join(folder, name), encoding="utf-8", errors="replace") as f:
                    out.write(f"\n--- {name} ---\n\n{f.read().strip()}\n")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: third-party-notices.py <package>")
    main(sys.argv[1])
