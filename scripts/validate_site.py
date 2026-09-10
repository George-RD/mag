#!/usr/bin/env python3
"""Validate shipped static pages, local links, assets and case-safe paths."""
from html.parser import HTMLParser
from pathlib import Path
import struct
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET

PAGES = ('index.html', 'demos/index.html', 'demos/retrieval-pipeline.html',
         'demos/cross-tool-handoff.html', 'demos/abstention.html', 'demos/capture-notes.html')
ASSETS = ('benchmark.svg', 'context-flow.svg', 'favicon.svg', 'pipeline.svg',
          'readme-header.svg', 'og-card.png', 'demo-player.js')


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.lang = self.title = self.description = self.in_title = False
        self.ids = set()
        self.links = []
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'html' and attrs.get('lang'):
            self.lang = True
        if tag == 'title':
            self.in_title = True
        if tag == 'meta' and attrs.get('name') == 'description' and attrs.get('content'):
            self.description = True
        if attrs.get('id'):
            self.ids.add(attrs['id'])
        for key in ('href', 'src'):
            if attrs.get(key):
                self.links.append(attrs[key])

    def handle_endtag(self, tag):
        if tag == 'title':
            self.in_title = False

    def handle_data(self, text):
        if self.in_title and text.strip():
            self.title = True


def validate_site(site):
    site = Path(site).resolve()
    errors, names, pages = [], {}, {}
    for relative in (*PAGES, *('assets/' + item for item in ASSETS)):
        if not (site / relative).is_file():
            errors.append('missing file: ' + relative)
    for path in sorted(site.rglob('*')):
        relative = str(path.relative_to(site))
        if relative.casefold() in names:
            errors.append('case-insensitive collision: ' + relative)
        names[relative.casefold()] = relative
        if path.suffix == '.html':
            page = Page(path.read_text(encoding='utf-8'))
            pages[path] = page
            if not all((page.lang, page.title, page.description)):
                errors.append(relative + ': missing page metadata')
    for path, page in pages.items():
        for link in page.links:
            url = urlsplit(link)
            if url.scheme or url.netloc:
                continue
            target = (path.parent / unquote(url.path)).resolve() if url.path else path
            if not target.is_relative_to(site):
                errors.append(str(path.relative_to(site)) + ': link outside site: ' + link)
                continue
            if not target.exists():
                errors.append(str(path.relative_to(site)) + ': broken link: ' + link)
            elif url.fragment and target in pages and unquote(url.fragment) not in pages[target].ids:
                errors.append(str(path.relative_to(site)) + ': broken fragment: ' + link)
    for svg in sorted((site / 'assets').glob('*.svg')):
        try:
            ET.parse(svg)
        except ET.ParseError as error:
            errors.append(svg.name + ': invalid SVG: ' + str(error))
    pipeline = site / 'assets/pipeline.svg'
    if pipeline.is_file():
        markup = pipeline.read_text(encoding='utf-8')
        for required in ('<title', '<desc', 'prefers-reduced-motion'):
            if required not in markup:
                errors.append('pipeline.svg: missing ' + required)
        for banned in ('<animate', 'xlink:href', '<image'):
            if banned in markup:
                errors.append('pipeline.svg: unsupported ' + banned)
    image = site / 'assets/og-card.png'
    if image.is_file():
        data = image.read_bytes()
        if len(data) < 2000 or data[:8] != b'\x89PNG\r\n\x1a\n':
            errors.append('og-card.png: invalid PNG payload')
        elif struct.unpack('>II', data[16:24]) != (1200, 630):
            errors.append('og-card.png: expected 1200x630')
    return errors


if __name__ == '__main__':
    findings = validate_site(Path(__file__).resolve().parents[1] / 'site')
    if findings:
        raise SystemExit('\n'.join(findings))
    print('Static site validation passed')
