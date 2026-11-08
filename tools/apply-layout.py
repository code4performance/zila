#!/usr/bin/env python3
"""One-off: writes the shared page frame (skip link, header with the icon and the menu toggle,
footer, favicon link) into every page of docs/. The navigation list and the content of each page are
kept as they are. Running it again changes nothing. tests/docs.rs keeps the pages identical afterwards.

    python tools/apply-layout.py
"""

import glob
import os
import re
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
DOCS = os.path.join(ROOT, "docs")


def description_field(name):
    with open(os.path.join(DOCS, "assets", "description.txt"), encoding="utf-8") as f:
        for line in f:
            if line.startswith(name + ":"):
                return line.split(":", 1)[1].strip()
    raise SystemExit("docs/assets/description.txt has no %s" % name)


def frame(html, tagline):
    # the navigation of the page, unchanged
    nav = re.search(r'<nav class="site-nav">.*?</nav>', html, re.S)
    if not nav:
        raise SystemExit("a page has no site navigation")
    header = (
        '<a class="skip" href="#content">Skip to content</a>\n'
        '  <header class="site-header">\n'
        '    <a class="brand" href="index.html"><img src="assets/zila.svg" alt="" width="28" height="28"><span>Zila Lang</span><small>docs</small></a>\n'
        '    <input type="checkbox" id="menu" class="menu-toggle">\n'
        '    <label for="menu" class="menu-button">Menu</label>\n'
        "    " + nav.group(0) + "\n"
        "  </header>"
    )
    html = re.sub(r'(<a class="skip" href="#content">Skip to content</a>\s*)?<header class="site-header">.*?</header>', lambda m: header, html, count=1, flags=re.S)
    html = re.sub(r"<main(?: id=\"content\")?>", '<main id="content">', html, count=1)
    footer = (
        '<footer class="site-footer">\n'
        "    Zila Lang &middot; " + tagline + " The examples are checked against <code>examples/*.br</code>; the grammar is <code>zila.ebnf</code>.\n"
        "  </footer>"
    )
    html = re.sub(r'<footer class="site-footer">.*?</footer>', lambda m: footer, html, count=1, flags=re.S)
    icon = '<link rel="icon" href="assets/zila.svg" type="image/svg+xml">'
    if icon not in html:
        html = html.replace('<link rel="stylesheet" href="styles.css">', '<link rel="stylesheet" href="styles.css">\n  ' + icon, 1)
    return html


def main():
    tagline = description_field("tagline")
    changed = 0
    for path in sorted(glob.glob(os.path.join(DOCS, "*.html"))):
        with open(path, encoding="utf-8", newline="") as f:
            raw = f.read()
        crlf = "\r\n" in raw
        html = raw.replace("\r\n", "\n")
        new = frame(html, tagline)
        if new != html:
            with open(path, "w", encoding="utf-8", newline="") as f:
                f.write(new.replace("\n", "\r\n") if crlf else new)
            changed += 1
    print("pages rewritten: %d" % changed)


if __name__ == "__main__":
    sys.exit(main())
