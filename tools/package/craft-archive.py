# SPDX-License-Identifier: MIT
"""Write a release archive that is deliberately wrong in one named way.

tools/check-packager.sh uses this to ask whether the gate refuses an archive
that did not come from tools/package-release.sh. A gate that can only fail its
own packager's output is a gate that passes anything else, and an absolute
path, a parent traversal or a symlink cannot be tested by extracting first:
those are properties of the archive and each of them writes outside the
directory the extraction was told to use.

Not a packager. Nothing here is ever run for a real release, and every archive
it writes is expected to be refused.

Usage: craft-archive.py TREE ROOT OUTPUT DEFORMATION
"""

import os
import sys
import tarfile

MTIME = 0


def entries(tree, root):
    """Every path under TREE/ROOT, relative to TREE, in normalised order."""
    found = []
    for base, directories, files in os.walk(os.path.join(tree, root)):
        for name in list(directories) + files:
            full = os.path.join(base, name)
            found.append(os.path.relpath(full, tree).replace(os.sep, "/"))
    return sorted(found)


def add(archive, tree, name, arcname=None, mode=None, mtime=MTIME):
    full = os.path.join(tree, name)
    info = archive.gettarinfo(full, arcname=arcname or name)
    # gettarinfo strips a leading slash, which is exactly the deformation one
    # of these cases is about, so the name is put back afterwards.
    if arcname is not None:
        info.name = arcname
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    info.mtime = mtime
    if mode is not None:
        info.mode = mode
    if info.isfile():
        with open(full, "rb") as handle:
            archive.addfile(info, handle)
    else:
        archive.addfile(info)


def main():
    tree, root, output, deformation = sys.argv[1:5]
    names = entries(tree, root)

    with tarfile.open(output, "w:gz", format=tarfile.USTAR_FORMAT) as archive:
        if deformation == "no-exec-bit":
            for name in names:
                mode = None if os.path.isdir(os.path.join(tree, name)) else 0o644
                add(archive, tree, name, mode=mode)
        elif deformation == "two-roots":
            for name in names:
                add(archive, tree, name)
            second = tarfile.TarInfo("a-second-root/README")
            second.size = 0
            second.mtime = MTIME
            archive.addfile(second)
        elif deformation == "unsorted":
            for name in reversed(names):
                add(archive, tree, name)
        elif deformation == "varying-mtimes":
            for index, name in enumerate(names):
                add(archive, tree, name, mtime=MTIME + index * 86400)
        elif deformation == "absolute-path":
            for name in names:
                add(archive, tree, name)
            add(archive, tree, names[-1], arcname="/etc/ferritecad-was-here")
        elif deformation == "parent-traversal":
            for name in names:
                add(archive, tree, name)
            add(archive, tree, names[-1], arcname=root + "/../escaped")
        elif deformation == "symlink-payload":
            victim = next(n for n in names if os.path.isfile(os.path.join(tree, n)))
            for name in names:
                if name == victim:
                    link = tarfile.TarInfo(name)
                    link.type = tarfile.SYMTYPE
                    link.linkname = "/usr/lib/somewhere-else"
                    link.mtime = MTIME
                    archive.addfile(link)
                else:
                    add(archive, tree, name)
        else:
            raise SystemExit("craft-archive: unknown deformation " + deformation)


if __name__ == "__main__":
    main()
