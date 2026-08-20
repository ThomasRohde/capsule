# Removing the programme overlay

The installer adds only the files listed in `PACKAGE_MANIFEST.json` under the
package's `overlay/` prefix. It does not modify pre-existing files.

Before removing the overlay from a working Capsule checkout:

1. preserve milestone results, evidence and accepted ADRs that have become part of
   the implementation history;
2. inspect `git status` and commit or archive intended work;
3. remove only files originally installed by the overlay that are still unmodified;
4. do not remove production code or repository documentation created during the
   implementation milestones.

For a clean, unused installation, compare the checkout files with
`PACKAGE_MANIFEST.json` hashes and delete the matching overlay paths. Git is the
preferred recovery mechanism; this package deliberately does not include an
automatic destructive uninstaller.
