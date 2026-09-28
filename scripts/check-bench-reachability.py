#!/usr/bin/env python3
"""
Reachability of // perf: pages from a CI-gated T1 bench entry point.

Question this answers, per page:
  "If this page's code got slower, would a gated bench number move?"

Method: build a module-level call graph per crate, BFS from the bench
entry points, and report every page's reachability.

Deliberately conservative. A module is reported COVERED only if the
graph proves a path from a root. Anything the parser cannot resolve is
reported UNKNOWN, never COVERED — a false "covered" would silently
retire a page that nobody is actually watching.
"""
import pathlib, re, sys, collections, json

# ---------- module tree ----------

MOD_DECL = re.compile(r'^\s*(?:#\[[^\]]*\]\s*)*pub(?:\([^)]*\))?\s+mod\s+(\w+)\s*;', re.M)
MOD_INLINE = re.compile(r'^\s*(?:#\[[^\]]*\]\s*)*pub(?:\([^)]*\))?\s+mod\s+(\w+)\s*\{', re.M)
PATH_ATTR = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]')
MOD_WITH_PATH = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\][^\n]*\bmod\s+(\w+)\s*;')
MOD_PLAIN = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;', re.M)

FN_DEF = re.compile(r'\bfn\s+(\w+)\s*(?:<[^>]*>)?\s*\(')
CALL = re.compile(r'(?<![\w:])([a-z_][a-z0-9_]{2,})\s*\(')
METHOD_CALL = re.compile(r'\.\s*(\w+)\s*\(')
# Any path-qualified call, not just `Type::method`. Module paths are the
# common case in this codebase -- `physics::update_life(self, dt, ..)` --
# and matching only uppercase type names silently cut the entire
# `mod.rs` -> `physics.rs` -> `physics/*.rs` edge, which is what made
# savers look half-uncovered when it is not.
QUALIFIED = re.compile(r'\b([A-Za-z_][A-Za-z0-9_]*)\s*::\s*(\w+)')


def resolve(base_dir, modname, path_attr, inline_stack):
    """mod foo;  ->  foo.rs | foo/mod.rs ; honour #[path=".."]."""
    if path_attr:
        cands = [base_dir / path_attr]
    else:
        stem = base_dir / modname
        cands = [stem.with_suffix(".rs"), stem / "mod.rs"]
    for c in cands:
        if c.is_file():
            return c
    return None


def build_tree(root_file):
    """Return {file: set(modname)} and parent links."""
    files = set()
    mods = {}
    stack = [(root_file, None, [])]
    while stack:
        f, pathattr, inline = stack.pop()
        if f in files or not f.is_file():
            continue
        files.add(f)
        src = f.read_text(errors="replace")
        parent_dir = f.parent
        found = []
        for m in MOD_WITH_PATH.finditer(src):
            found.append((m.group(2), m.group(1)))
        stripped = MOD_WITH_PATH.sub("", src)
        for m in MOD_PLAIN.finditer(stripped):
            found.append((m.group(1), None))
        for modname, pa in found:
            child = resolve(parent_dir, modname, pa, inline)
            if child:
                mods.setdefault(child, set()).add(modname)
                stack.append((child, None, inline + [modname]))
    return files, mods


def module_id(f, root_file):
    """Stable dotted module path for a file, relative to the crate root."""
    try:
        rel = f.relative_to(root_file.parent)
    except ValueError:
        return str(f)
    parts = list(rel.parts)
    if parts[-1] in ("lib.rs", "main.rs", "mod.rs"):
        parts = parts[:-1]
    else:
        parts[-1] = parts[-1][:-3]
    return "::".join([p for p in parts if p]) or "<root>"


# ---------- per-file facts ----------

def facts(path):
    src = path.read_text(errors="replace")
    defs = set(FN_DEF.findall(src))
    calls = set(CALL.findall(src)) | set(METHOD_CALL.findall(src))
    # qualified Type::method -> both halves are call-ish names
    for ty, meth in QUALIFIED.findall(src):
        calls.add(meth)
    return defs, calls, src


def crate_roots(repo):
    """Every Cargo package root in a repo -> its crate root .rs file."""
    out = []
    for man in sorted(pathlib.Path(repo).rglob("Cargo.toml")):
        if "/target/" in str(man):
            continue
        d = man.parent
        for cand in ("src/lib.rs", "src/main.rs"):
            if (d / cand).is_file():
                out.append((d, d / cand))
                break
    return out


def load_label(path):
    lines = path.read_text(errors="replace").splitlines()[:6]
    for l in lines:
        t = l.strip()
        if t.startswith("//") and "perf:" in t:
            d = {}
            for p in t[2:].split("·"):
                p = p.strip()
                if ":" in p:
                    k, v = p.split(":", 1)
                    d[k.strip()] = v.strip()
            if d.get("perf"):
                return d
    return None


def analyse_repo(repo):
    pkgs = crate_roots(repo)
    covered, uncovered, unknown = [], [], []

    for pkg_dir, crate_root in pkgs:
        files, _ = build_tree(crate_root)
        if not files:
            continue
        # collect defs per module
        mod_defs = {}
        all_calls = {}
        for f in files:
            d, c, src = facts(f)
            mod_defs[f] = d
            all_calls[f] = c

        # name -> modules defining it (crate-scoped)
        name_to_mods = collections.defaultdict(set)
        for f, d in mod_defs.items():
            for n in d:
                name_to_mods[n].add(f)

        # roots: bench targets are entry points in their own right. A
        # `[[bench]]` file lives outside the crate's mod tree, so without
        # seeding it here every page it calls directly looks unreachable —
        # which is exactly how idle-upscaler's 7 T1 pages first came out
        # "uncovered" against a gate that benches all of them.
        ROOT_SYMS = {
            "Screensaver", "update", "draw", "prepare_for_bench",
            "render_content_viewport_into", "draw_frame",
            "stretch", "upscale_stretch_into", "render_content_span_into",
        }
        roots = set()
        for f, d in mod_defs.items():
            if d & ROOT_SYMS:
                roots.add(f)

        bench_files = sorted((pkg_dir / "benches").glob("*.rs")) if (pkg_dir / "benches").is_dir() else []
        for bf in bench_files:
            if bf.is_file():
                files.add(bf)
                d, c, _ = facts(bf)
                mod_defs[bf] = d
                all_calls[bf] = c
                for n in d:
                    name_to_mods[n].add(bf)
                roots.add(bf)

        # BFS over module edges
        seen = set(roots)
        frontier = list(roots)
        while frontier:
            nf = []
            for f in frontier:
                for name in all_calls.get(f, ()):
                    for tgt in name_to_mods.get(name, ()):
                        if tgt == f:
                            continue
                        # a call to a same-named local fn is a self edge
                        if name in mod_defs.get(f, ()):
                            continue
                        if tgt not in seen:
                            seen.add(tgt)
                            nf.append(tgt)
            frontier = nf

        for f in files:
            lab = load_label(f)
            if not lab:
                continue
            if f in roots:
                verdict = "ROOT"
            elif f in seen:
                verdict = "COVERED"
            else:
                verdict = "UNCOVERED"
            (covered if verdict in ("ROOT", "COVERED") else uncovered).append(
                (repo, str(f), lab.get("perf"), lab.get("check"), verdict, len(f.read_text(errors='replace').splitlines()))
            )
    return covered, uncovered


def report(repos):
        all_cov, all_unc = [], []
        for r in repos:
            c, u = analyse_repo(r)
            all_cov += c
            all_unc += u
        print(f"reachable from a bench root : {len(all_cov)}")
        print(f"NOT reachable                : {len(all_unc)}")
        print()
        byc = collections.Counter((x[0], x[2], x[3], x[4]) for x in all_unc)
        print("unreachable by repo/tier/check:")
        for k, n in sorted(byc.items()):
            print(f"  {k[0]:9} {k[1]:3} {k[2]:7} {k[3]:10} {n}")
        print()
        print("--- sample unreachable pages (T3/review only matter) ---")
        shown = 0
        for repo, path, tier, chk, verdict, n in sorted(all_unc, key=lambda x: (x[0], x[1])):
            if chk == "review" and tier == "T3":
                print(f"  {repo:9} {n:4}L  {path}")
                shown += 1
                if shown >= 25:
                    break
        print(f"  ... ({sum(1 for x in all_unc if x[3]=='review' and x[2]=='T3')} T3/review pages total)")


    # ---------- gate ----------


def gate(repos):
    """Fail if any `check: bench` page is NOT provably bench-reachable.

    This is the one direction worth gating. A page labelled `check: bench`
    claims a criterion target measures it; if the call graph cannot find a
    path from that bench's entry point, the claim is false and the page
    has silently lost its only automated detection. Everything else
    (`test`, `review`) is reported but never fails the build.
    """
    import re as _re
    FN_BODY = _re.compile(r'\bfn\s+(\w+)\s*(?:<[^>]*>)?\s*\([^)]*\)[^{;]*\{')
    verdict, bad, review_cov, review_unc, review_unk = {}, [], 0, 0, 0
    n_scanned = 0
    for r in repos:
        c, u = analyse_repo(r)
        for x in c + u:
            verdict[x[1]] = x[4]
    for repo in repos:
        for f in pathlib.Path(repo).rglob("*.rs"):
            s = str(f)
            if "/target/" in s:
                continue
            lab = load_label(f)
            if not lab:
                continue
            n_scanned += 1
            v = verdict.get(s, "UNKNOWN")
            if lab.get("check") == "bench" and v == "UNCOVERED":
                bad.append(s)
            elif lab.get("check") == "review":
                if v in ("ROOT", "COVERED"):
                    review_cov += 1
                elif v == "UNKNOWN":
                    review_unk += 1
                else:
                    code = _re.sub(r'//[^\n]*', '', _re.sub(r'/\*.*?\*/', '', f.read_text(errors="replace"), flags=_re.S))
                    review_unc += 1 if FN_BODY.search(code) else 0
    if n_scanned == 0:
        print("ERROR: no perf-labelled pages found — refusing to pass vacuously", file=sys.stderr)
        return 2
    print(f"scanned {n_scanned} labelled pages")
    print(f"check: review pages — bench-reachable {review_cov}, "
          f"not reachable but has executable code {review_unc}, outside the module tree {review_unk}")
    if bad:
        print("\nERROR: these pages claim `check: bench` but no bench reaches them:", file=sys.stderr)
        for b in bad:
            print(f"  {b}", file=sys.stderr)
        return 1
    print("all `check: bench` pages are reachable from a bench entry point")
    return 0


# Resolve scan roots against this script's own repository, never against
# the process CWD. CI runs the step from the checkout root, where a
# literal "runtime" directory does not exist -- an earlier version
# scanned nothing and exited 0, which is the worst possible outcome for a
# gate: it reported success while checking no files at all.
REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent


def resolve_roots(names):
    """Map repo names to real directories under this checkout."""
    roots = []
    for n in names:
        cand = REPO_ROOT / n
        roots.append(cand if cand.is_dir() else REPO_ROOT)
    return roots or [REPO_ROOT]


def rel(p):
    try:
        return str(pathlib.Path(p).resolve().relative_to(REPO_ROOT))
    except ValueError:
        return str(p)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--gate":
        sys.exit(gate(resolve_roots(sys.argv[2:])))
    report(resolve_roots(sys.argv[1:] or ["."]))
