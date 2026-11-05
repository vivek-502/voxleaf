#!/usr/bin/env python3
"""
Migrate the ported React sources off Next.js onto react-router-dom.

The VoiceBook web app was a Next.js App Router project. Its components only ever
reached for four Next.js APIs — `next/link`, `useRouter`, `usePathname` and
`useSearchParams` — and read the search params but never wrote them, so the
mapping to react-router is mechanical:

    next/link                -> react-router-dom Link,  href=  -> to=
    useRouter().push(path)   -> navigate(path)
    usePathname()            -> useLocation().pathname
    useSearchParams()        -> [searchParams] = useSearchParams()
"""
import re
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / "src"
files = [p for p in ROOT.rglob("*.ts*") if "node_modules" not in p.parts]

changed = []
for path in files:
    text = path.read_text(encoding="utf-8")
    original = text

    # 1. The 'use client' directive is a Next.js bundler hint; React + Vite needs it.
    text = re.sub(r"^'use client';\n+", "", text)
    text = re.sub(r"^\"use client\";\n+", "", text, flags=re.M)

    # 2. next/link -> react-router-dom Link.
    text = re.sub(
        r"^import Link from 'next/link';\n",
        "import { Link } from 'react-router-dom';\n",
        text,
        flags=re.M,
    )

    # 3. next/navigation hooks -> react-router-dom equivalents.
    text = text.replace(
        "import { usePathname, useRouter, useSearchParams } from 'next/navigation';",
        "import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';",
    )
    text = text.replace(
        "import { usePathname } from 'next/navigation';",
        "import { useLocation } from 'react-router-dom';",
    )
    text = text.replace(
        "import { useRouter, useSearchParams } from 'next/navigation';",
        "import { useNavigate, useSearchParams } from 'react-router-dom';",
    )
    text = text.replace(
        "import { useRouter } from 'next/navigation';",
        "import { useNavigate } from 'react-router-dom';",
    )

    # 4. Call sites.
    text = re.sub(r"const router = useRouter\(\);", "const navigate = useNavigate();", text)
    text = re.sub(r"const pathname = usePathname\(\);", "const pathname = useLocation().pathname;", text)
    # react-router returns a tuple; only the read half is used across the app.
    text = re.sub(
        r"const searchParams = useSearchParams\(\);",
        "const [searchParams] = useSearchParams();",
        text,
    )
    text = re.sub(r"\brouter\.push\(", "navigate(", text)
    text = re.sub(r"\brouter\.replace\(", "navigate(", text)

    # 5. <Link href=...> -> <Link to=...> (multi-line aware).
    text = re.sub(r"<Link(\s+)href=", r"<Link\1to=", text)

    if text != original:
        path.write_text(text, encoding="utf-8")
        changed.append(str(path.relative_to(ROOT)))

print("migrated %d files" % len(changed))
for c in sorted(changed):
    print("  ", c)

# Sanity: nothing Next.js-specific should survive.
bad = []
# Directives are checked at line start: "'use client'" quoted in a doc comment
# is prose, not a directive, and a bare substring scan would fail on it. The
# Next import paths are checked anywhere — they only ever appear as imports or
# genuine references, either of which must be gone.
for path in ROOT.rglob("*.ts*"):
    body = path.read_text(encoding="utf-8")
    for lineno, line in enumerate(body.splitlines(), 1):
        stripped = line.strip()
        if stripped in ("'use client'", '"use client"', "'server-only'", '"server-only"'):
            bad.append("%s:%d -> %s" % (path.relative_to(ROOT), lineno, stripped))
        for needle in ("next/link", "next/navigation"):
            if needle in line and not line.lstrip().startswith(("*", "//", "/*")):
                bad.append("%s:%d -> %s" % (path.relative_to(ROOT), lineno, needle))
if bad:
    print("\nREMAINING Next.js references:")
    for b in bad:
        print("  ", b)
    sys.exit(1)
print("\nno Next.js-specific references remain")
