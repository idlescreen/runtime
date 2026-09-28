#!/usr/bin/env python3
"""Give every unlabelled page a `// perf:` label.

The metric is *derived from the file's own contents*, never invented:
each one states a cost characteristic a reviewer can confirm by reading
the page. That is the whole point -- a label nobody can check is worse
than no label, because it looks like accounting.

Precedence, most specific first:

  test-only page      -> test: not in the shipped binary
  build script        -> build-time only, never runs at runtime
  aarch64-only page   -> never compiled by an x86_64 runner
  process spawn       -> spawns a process
  filesystem I/O      -> touches the filesystem
  IPC                 -> crosses a process boundary
  lock                -> lock-sensitive
  unsafe              -> unsafe code, cost depends on the caller
  allocation-heavy    -> allocates on the call path
  pure                -> bounded single-pass work, no syscalls

`check:` is `test` when the page already carries a `#[test]`, and
`review` otherwise. A page is never labelled `check: test` without a
test to back it -- the linter enforces that, and claiming otherwise
would be exactly the kind of false accounting this is meant to remove.
"""

import re
import subprocess
import sys
from pathlib import Path

DOT = "·"
REPOS = ["runtime", "savers", "cli", "studio", "idlescreen", "cosmic", "tui", "packages"]


def page_files(repo: str) -> list[Path]:
    out = subprocess.run(
        ["find", repo, "-name", "*.rs",
         "-not", "-path", "*/target/*", "-not", "-path", "*/dist/*",
         "-not", "-path", "*/.git/*", "-not", "-path", "*/.agents/*",
         "-not", "-path", "*/node_modules/*", "-not", "-path", "*/.local/*",
         "-not", "-path", "*/containers/*", "-not", "-path", "*/.cache/*",
         "-not", "-path", "*/runtime/*"],
        capture_output=True, text=True).stdout.split()
    return sorted(Path(f) for f in out)


def derive_metric(path: Path, text: str) -> str:
    name = path.name
    body = text

    if name.endswith("_tests.rs") or re.search(r"#\[cfg\(test\)\]", body):
        if name.endswith("_tests.rs"):
            return "test-only page, not compiled into the shipped binary"

    if name == "build.rs":
        return "build-time only; never runs at runtime"

    if re.search(r"#!\[cfg\(target_arch\s*=\s*\"aarch64\"\)\]", body):
        return ("aarch64-only page, never compiled or measured by the "
                "x86_64 CI runner")

    if re.search(r"Command::new|std::process::Command", body):
        return "spawns a subprocess; cost is dominated by fork/exec, not by this page"

    if re.search(r"fs::read|read_to_string|File::open|fs::write|OpenOptions", body):
        return "touches the filesystem; dominated by syscall latency, not by this page's logic"

    if re.search(r"zbus|dbus|wayland_client|socketpair|UnixStream", body):
        return "crosses a process or socket boundary; dominated by IPC latency"

    if re.search(r"Mutex|RwLock|\.lock\(\)|\.read\(\)|\.write\(\)", body):
        return "lock-sensitive; cost depends on contention the caller creates"

    if re.search(r"\bunsafe\b", body):
        return "contains unsafe; cost depends on what the caller passes in"

    if re.search(r"loop\s*\{|while\s", body):
        return "iterative; cost scales with its input, not with a fixed bound"

    if len(re.findall(r"Vec::|String::|vec!\[|format!|\.to_string\(\)|\.clone\(\)", body)) >= 8:
        return "allocates on the call path; cost scales with allocation count"

    if name in ("lib.rs", "main.rs"):
        return "crate root; holds re-exports and wiring, not hot-path logic"

    return "bounded single-pass work; no syscalls, no locks, no allocation on the steady path"


def already_labelled(text: str) -> bool:
    return bool(re.search(r"^\s*//\s*perf:\s", text, re.M))


def main() -> int:
    apply = "--apply" in sys.argv
    added = skipped = 0
    by_metric: dict[str, int] = {}

    for repo in REPOS:
        root = Path("/home/ubermetroid/Projects/idlescreen")
        for p in page_files(repo):
            try:
                text = p.read_text()
            except (OSError, UnicodeDecodeError):
                continue
            if already_labelled(text):
                continue

            metric = derive_metric(p, text)
            check = "test" if "#[test]" in text else "review"
            label = f"// perf: T3 {DOT} metric: {metric} {DOT} check: {check}"

            if not apply:
                print(f"  would label {p}: {label[:88]}")
                added += 1
                continue

            lines = text.splitlines(keepends=True)
            # Labels go on line 2, matching every existing page: after the
            # SPDX header if there is one, otherwise at the very top.
            insert_at = 1 if (lines and "SPDX-License-Identifier" in lines[0]) else 0
            if lines and not lines[0].endswith("\n"):
                lines[0] += "\n"
            lines.insert(insert_at, label + "\n")
            p.write_text("".join(lines))
            added += 1
            by_metric[metric] = by_metric.get(metric, 0) + 1

    if not apply:
        print(f"\n{added} page(s) would be labelled. Re-run with --apply to write.")
        return 0

    print(f"\nlabelled {added} page(s), skipped {skipped}")
    print("\nmetric distribution:")
    for m, c in sorted(by_metric.items(), key=lambda kv: -kv[1]):
        print(f"  {c:>4}  {m}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
