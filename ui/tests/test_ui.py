"""Static checks for the demo UI (stdlib only).

Every HTML page must parse, carry the page contract (lang, charset,
viewport, CSP, title, one h1, a skip link to main), label every control,
link only to files that exist, load scripts and styles only from this
directory or from the allowed CDNs at an exact version, and use no inline
script, inline handler, or inline style (the CSP forbids them).

Run: python -m pytest ui/tests -q   (or python -m unittest discover ui/tests)
"""

import html.parser
import pathlib
import re
import unittest
import urllib.parse

UI = pathlib.Path(__file__).resolve().parent.parent
PAGES = sorted(UI.glob("*.html"))
ASSETS = sorted(UI.glob("assets/*.js")) + sorted(UI.glob("assets/*.css"))

EXPECTED_PAGES = {
    "index.html",
    "pay.html",
    "wallet.html",
    "ledger.html",
    "verify.html",
    "evidence.html",
}

# Hosts a page may load code or styles from, and only at an exact version.
CDN_HOSTS = {"cdnjs.cloudflare.com", "cdn.jsdelivr.net"}
EXACT_VERSION = re.compile(r"[@/]v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.]+)?(?:/|$)")
# Hosts the scripts may call at runtime (read-only NEAR RPC).
RUNTIME_HOSTS = {
    "test.rpc.fastnear.com",
    "free.rpc.fastnear.com",
    "archival-rpc.testnet.fastnear.com",
    "archival-rpc.mainnet.fastnear.com",
}
# Hosts that ordinary outbound links may point to.
LINK_HOSTS = {"github.com", "nearblocks.io", "testnet.nearblocks.io", "docs.near.org"}
# Reserved documentation names (RFC 2606 / RFC 6761) used in sample data.
RESERVED = re.compile(r"(^|\.)(example\.(org|com|net)|example|test)$")

URL_IN_TEXT = re.compile(r"https?://([A-Za-z0-9.-]+)")


class Page(html.parser.HTMLParser):
    """Collects the elements the checks need."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.tags = []  # (tag, attrs dict)
        self.text_by_index = {}
        self.ids = set()
        self.label_for = set()
        self.inline_scripts = 0
        self.title = ""
        self._in_title = False
        self._in_script = False
        self._open = []  # stack of (tag, index)
        self.doctype = False

    def handle_decl(self, decl):
        if decl.lower().strip() == "doctype html":
            self.doctype = True

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        index = len(self.tags)
        self.tags.append((tag, attrs))
        if "id" in attrs:
            self.ids.add(attrs["id"])
        if tag == "label" and "for" in attrs:
            self.label_for.add(attrs["for"])
        if tag == "title":
            self._in_title = True
        if tag == "script":
            self._in_script = True
        if tag not in {"meta", "link", "input", "br", "img", "hr", "source"}:
            self._open.append((tag, index))

    def handle_endtag(self, tag):
        if tag == "title":
            self._in_title = False
        if tag == "script":
            self._in_script = False
        while self._open:
            open_tag, _ = self._open.pop()
            if open_tag == tag:
                break

    def handle_data(self, data):
        if self._in_title:
            self.title += data
        if self._in_script and data.strip():
            self.inline_scripts += 1
        for _, index in self._open:
            self.text_by_index[index] = self.text_by_index.get(index, "") + data

    def find(self, tag):
        return [(i, a) for i, (t, a) in enumerate(self.tags) if t == tag]


def parse(path):
    parser = Page()
    parser.feed(path.read_text(encoding="utf-8"))
    parser.close()
    return parser


def host_of(url):
    return (urllib.parse.urlsplit(url).hostname or "").lower()


def is_external(url):
    return urllib.parse.urlsplit(url).scheme in {"http", "https"} or url.startswith("//")


class UiPagesTest(unittest.TestCase):
    def test_the_six_pages_exist(self):
        self.assertEqual({p.name for p in PAGES}, EXPECTED_PAGES)

    def test_page_contract(self):
        for path in PAGES:
            with self.subTest(page=path.name):
                page = parse(path)
                self.assertTrue(page.doctype, "missing <!doctype html>")
                html_tags = page.find("html")
                self.assertTrue(html_tags and html_tags[0][1].get("lang"), "missing lang")
                metas = [a for _, a in page.find("meta")]
                self.assertTrue(any("charset" in m for m in metas), "missing charset")
                self.assertTrue(
                    any(m.get("name") == "viewport" and "width=device-width" in m.get("content", "")
                        for m in metas),
                    "missing viewport",
                )
                self.assertTrue(
                    any(m.get("http-equiv") == "Content-Security-Policy" for m in metas),
                    "missing CSP",
                )
                self.assertTrue(page.title.strip(), "empty title")
                self.assertEqual(len(page.find("h1")), 1, "exactly one h1")
                mains = page.find("main")
                self.assertEqual(len(mains), 1)
                self.assertEqual(mains[0][1].get("id"), "main")
                skips = [a for _, a in page.find("a") if a.get("class") == "skip-link"]
                self.assertEqual(skips and skips[0].get("href"), "#main", "skip link")

    def test_every_control_is_labelled(self):
        for path in PAGES:
            page = parse(path)
            for tag in ("input", "select", "textarea"):
                for _, attrs in page.find(tag):
                    if attrs.get("type") == "hidden":
                        continue
                    with self.subTest(page=path.name, control=attrs.get("id")):
                        labelled = (
                            attrs.get("id") in page.label_for
                            or attrs.get("aria-label")
                            or attrs.get("aria-labelledby")
                        )
                        self.assertTrue(labelled, f"unlabelled <{tag}>")

    def test_buttons_have_a_type_and_a_name(self):
        for path in PAGES:
            page = parse(path)
            for index, attrs in page.find("button"):
                with self.subTest(page=path.name, button=attrs.get("id")):
                    self.assertIn(attrs.get("type"), {"button", "submit", "reset"})
                    name = page.text_by_index.get(index, "").strip() or attrs.get("aria-label")
                    self.assertTrue(name, "button without an accessible name")

    def test_ids_are_unique_and_label_targets_exist(self):
        for path in PAGES:
            page = parse(path)
            ids = [a["id"] for _, a in page.tags if "id" in a]
            with self.subTest(page=path.name):
                self.assertEqual(len(ids), len(set(ids)), "duplicate id")
                self.assertTrue(page.label_for <= page.ids, "label for= a missing id")

    def test_local_links_resolve(self):
        for path in PAGES:
            page = parse(path)
            for tag, attr in (("a", "href"), ("link", "href"), ("script", "src"), ("img", "src")):
                for _, attrs in page.find(tag):
                    target = attrs.get(attr)
                    if not target or is_external(target):
                        continue
                    with self.subTest(page=path.name, target=target):
                        file_part, _, fragment = target.partition("#")
                        if file_part:
                            self.assertTrue((UI / file_part).is_file(), "missing file")
                            if fragment:
                                self.assertIn(fragment, parse(UI / file_part).ids)
                        else:
                            self.assertIn(fragment, page.ids, "missing anchor")

    def test_navigation_is_complete_and_marks_the_current_page(self):
        for path in PAGES:
            page = parse(path)
            nav_links = [a for _, a in page.find("a") if a.get("href") in EXPECTED_PAGES]
            with self.subTest(page=path.name):
                self.assertEqual({a["href"] for a in nav_links}, EXPECTED_PAGES)
                current = [a["href"] for a in nav_links if a.get("aria-current") == "page"]
                self.assertEqual(current, [path.name])

    def test_resources_come_only_from_here_or_pinned_cdns(self):
        for path in PAGES:
            page = parse(path)
            loaded = [a.get("src") for _, a in page.find("script")]
            loaded += [a.get("href") for _, a in page.find("link")]
            loaded += [a.get("src") for t in ("img", "iframe", "source") for _, a in page.find(t)]
            for url in filter(None, loaded):
                if not is_external(url):
                    continue
                with self.subTest(page=path.name, url=url):
                    self.assertIn(host_of(url), CDN_HOSTS)
                    self.assertRegex(urllib.parse.urlsplit(url).path, EXACT_VERSION)

    def test_outbound_links_go_to_known_hosts(self):
        for path in PAGES:
            page = parse(path)
            for _, attrs in page.find("a"):
                href = attrs.get("href", "")
                if is_external(href):
                    with self.subTest(page=path.name, href=href):
                        self.assertIn(host_of(href), LINK_HOSTS)
                        self.assertTrue(href.startswith("https://"))

    def test_csp_compatible_markup(self):
        for path in PAGES:
            page = parse(path)
            with self.subTest(page=path.name):
                self.assertEqual(page.inline_scripts, 0, "inline script")
                for tag, attrs in page.tags:
                    self.assertNotIn("style", attrs, f"inline style on <{tag}>")
                    handlers = [k for k in attrs if k.startswith("on")]
                    self.assertFalse(handlers, f"inline handler on <{tag}>")
                self.assertEqual(page.find("style"), [], "<style> element")

    def test_csp_connect_sources_are_allowed_hosts(self):
        for path in PAGES:
            page = parse(path)
            csp = next(
                a["content"] for _, a in page.find("meta")
                if a.get("http-equiv") == "Content-Security-Policy"
            )
            directives = dict(
                (part.split()[0], part.split()[1:]) for part in csp.split(";") if part.strip()
            )
            with self.subTest(page=path.name):
                self.assertEqual(directives.get("script-src"), ["'self'"])
                self.assertEqual(directives.get("default-src"), ["'self'"])
                for source in directives.get("connect-src", []):
                    if source.startswith("https://"):
                        self.assertIn(host_of(source), RUNTIME_HOSTS)
                    elif source.startswith("http://"):
                        self.assertIn(host_of(source), {"127.0.0.1", "localhost"})
                    else:
                        self.assertEqual(source, "'self'")


class UiAssetsTest(unittest.TestCase):
    def test_assets_name_only_allowed_hosts(self):
        allowed = CDN_HOSTS | RUNTIME_HOSTS | LINK_HOSTS
        for path in ASSETS:
            text = path.read_text(encoding="utf-8")
            for host in URL_IN_TEXT.findall(text):
                host = host.lower()
                with self.subTest(asset=path.name, host=host):
                    self.assertTrue(host in allowed or RESERVED.search(host), host)

    def test_no_html_injection_sinks(self):
        for path in UI.glob("assets/*.js"):
            text = path.read_text(encoding="utf-8")
            with self.subTest(asset=path.name):
                for sink in ("innerHTML", "outerHTML", "insertAdjacentHTML", "document.write",
                             "eval(", "new Function"):
                    self.assertNotIn(sink, text)

    def test_mock_data_uses_reserved_names(self):
        text = (UI / "assets" / "samples.js").read_text(encoding="utf-8")
        accounts = set(re.findall(r'"([a-z0-9_-]+(?:\.[a-z0-9_-]+)*\.(?:testnet|near))"', text))
        for account in accounts:
            with self.subTest(account=account):
                self.assertTrue(".example." in account, "mock accounts must be .example.*")

    def test_every_referenced_asset_exists(self):
        for path in PAGES:
            text = path.read_text(encoding="utf-8")
            for ref in re.findall(r'(?:src|href)="(assets/[^"]+)"', text):
                with self.subTest(page=path.name, ref=ref):
                    self.assertTrue((UI / ref).is_file())


if __name__ == "__main__":
    unittest.main()
