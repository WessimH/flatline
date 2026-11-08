#!/usr/bin/env python3
"""Builds Flatline's headphone catalog (data/catalog.bin) from an AutoEq checkout.

Usage (stdlib only, run isolated):

    git clone --depth 1 --filter=blob:none --sparse https://github.com/jaakkopasanen/AutoEq.git /tmp/AutoEq
    git -C /tmp/AutoEq sparse-checkout set measurements targets dbtools
    python3 -I scripts/build_catalog.py /tmp/AutoEq

Everything that decides *which target and which bass shelf* a measurement gets
is read from AutoEq's own code, not re-typed here:

* ``dbtools/create_webapp_data.py``: the table behind autoeq.app. Each target
  lists the (source, form, rig) measurement groups it is *recommended* for, and
  its default ``bassBoost`` (105 Hz, Q 0.7, gain in dB). The web app picks the
  target of a measurement with exactly this table (``ApiClient.fetchTargets``).
* ``dbtools/update_result_indexes.py``: ``ResultPath.priorities``, AutoEq's
  ranking of measurement sources per form (which one is "recommended" when a
  headphone was measured by several sources). Used as our source ranking.
* ``dbtools/manufacturers.tsv``: AutoEq's manufacturer index (true name and
  aliases), used to split names into brand / model with AutoEq's own
  longest-prefix rule (``ManufacturerIndex.find``).
* ``measurements/<source>/name_index.tsv``: the rig of each measurement when the
  source has no per-rig sub-folders.

Output format (all little-endian), see ``src/catalog.rs`` for the reader:

    header (40 bytes)
      0  magic  b"FLC1"
      4  u16    format version (1)
      6  u16    grid points
      8  f32    grid start frequency (Hz)
     12  f32    grid points per octave
     16  u32    string count
     20  u32    entry count
     24  u32    target count
     28  u32    curve data length (bytes)
     32  u32    provenance string id
     36  u32    reserved (0)
    string offsets  (string count + 1) x u32, then the UTF-8 bytes
    entries         entry count x 32 bytes:
                      u32 name, brand, model, variant, source, rig  (string ids,
                          0xFFFFFFFF = none for variant / rig)
                      u8 form (0 over-ear, 1 in-ear, 2 earbud)
                      u8 rank (AutoEq source priority within the form, 0 = best)
                      u8 flags (bit 0: the rig is part of the key; bit 1: the
                         entry's model has a stock, no-variant measurement)
                      u8 target index
                      u32 curve offset
    targets         target count x 16 bytes:
                      u32 file name, u32 label, i16 bass (centi-dB), u16 0,
                      u32 curve offset
    curve data      per curve: zigzag LEB128 varints, first value then deltas,
                    in centi-dB, one value per grid point.

Entries are stored in the catalog's default order (see the "Default order"
comment in main()), which the app uses as the tie-break of every search.

Curves are sampled on f = f0 * 2^(i / per_octave) by linear interpolation in
log-frequency (clamped at the ends, like ``Curve::at`` in the app) and shifted
so their 300 Hz - 3 kHz mean is 0 dB.
"""

import argparse
import ast
import csv
import io
import math
import re
import struct
import subprocess
import sys
import unicodedata
from collections import Counter, OrderedDict, defaultdict
from pathlib import Path

FORMS = ('over-ear', 'in-ear', 'earbud')
FORM_CODE = {'over-ear': 0, 'in-ear': 1, 'earbud': 2}
NONE = 0xFFFFFFFF
F0 = 20.0
F_TOP = 20_000.0
ALIGN = (300.0, 3000.0)

# Innerfidelity and Headphone.com Legacy have no name_index.tsv: their AutoEq
# crawlers hard-code the rig (dbtools/innerfidelity_crawler.py and
# dbtools/headphonecom_crawler.py, `rig='HMS II.3'`).
CRAWLER_RIGS = {'Innerfidelity': 'HMS II.3', 'Headphone.com Legacy': 'HMS II.3'}

ZERO_WIDTH = dict.fromkeys(map(ord, '​‌‍⁠﻿'))


def log(*args):
    print(*args, file=sys.stderr)


# ------------------------------------------------------------ AutoEq tables --

def _str_arg(call):
    """'x' from TARGETS_PATH.joinpath('x')."""
    if isinstance(call, ast.Call) and call.args and isinstance(call.args[0], ast.Constant):
        return call.args[0].value
    raise ValueError('unexpected target file expression')


def read_webapp_targets(repo):
    """Target table of autoeq.app, parsed from dbtools/create_webapp_data.py."""
    path = repo / 'dbtools' / 'create_webapp_data.py'
    tree = ast.parse(path.read_text(encoding='utf-8'))
    targets = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Dict):
            continue
        keys = [k.value for k in node.keys if isinstance(k, ast.Constant)]
        if 'file' not in keys or 'bassBoost' not in keys:
            continue
        item = {}
        for k, v in zip(node.keys, node.values):
            if k.value == 'file':
                item['file'] = re.sub(r'\.csv$', '', _str_arg(v))
            else:
                item[k.value] = ast.literal_eval(v)
        targets.append(item)
    if not targets:
        raise SystemExit(f'no target table found in {path}')
    return targets


def read_priorities(repo):
    """ResultPath.priorities from dbtools/update_result_indexes.py, best first."""
    path = repo / 'dbtools' / 'update_result_indexes.py'
    tree = ast.parse(path.read_text(encoding='utf-8'))
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'priorities' for t in node.targets):
            value = node.value
            reverse = False
            if isinstance(value, ast.Subscript):  # [...][::-1]
                sl = value.slice
                reverse = isinstance(sl, ast.Slice) and sl.step is not None and ast.literal_eval(sl.step) == -1
                value = value.value
            items = ast.literal_eval(value)
            return list(reversed(items)) if reverse else list(items)
    raise SystemExit(f'no priorities found in {path}')


def read_manufacturers(repo):
    """Lower-case alias -> true manufacturer name (dbtools/manufacturers.tsv)."""
    index = {}
    with open(repo / 'dbtools' / 'manufacturers.tsv', encoding='utf-8') as fh:
        for line in fh:
            names = [n.strip() for n in line.rstrip('\n').split('\t') if n.strip()]
            if names:
                for n in names:
                    index[clean(n).lower()] = names[0]
    return index


def read_name_index(path):
    """name -> list of (form, rig) rows from a measurements/<source>/name_index.tsv."""
    rows = defaultdict(list)
    if not path.exists():
        return rows
    with open(path, encoding='utf-8', newline='') as fh:
        for row in csv.DictReader(fh, delimiter='\t'):
            name = clean(row.get('name') or '')
            if name:
                rows[name].append(((row.get('form') or '').strip(), clean(row.get('rig') or '')))
    return rows


def autoeq_revision(repo):
    rev, date = None, None
    try:
        out = subprocess.run(['git', '-C', str(repo), 'log', '-1', '--format=%H %cs'],
                             capture_output=True, text=True, check=True).stdout.split()
        rev, date = out[0], out[1]
    except (OSError, subprocess.CalledProcessError, IndexError):
        head = repo / '.git' / 'HEAD'
        if head.exists():
            ref = head.read_text().strip()
            if ref.startswith('ref: '):
                ref_path = repo / '.git' / ref[5:]
                rev = ref_path.read_text().strip() if ref_path.exists() else None
            else:
                rev = ref
    s = 'AutoEq'
    if rev:
        s += f' {rev[:12]}'
    if date:
        s += f' ({date})'
    return s


# ------------------------------------------------------------------ names --

def clean(s):
    s = unicodedata.normalize('NFC', s).translate(ZERO_WIDTH)
    return re.sub(r'\s+', ' ', s).strip()


# Letters NFKD does not decompose (kept in step with fold_latin in src/catalog.rs).
FOLD_EXTRA = {'ø': 'o', 'æ': 'ae', 'œ': 'oe', 'ß': 'ss', 'đ': 'd', 'ð': 'd', 'ł': 'l', 'þ': 'th', 'ı': 'i'}


def fold(s):
    """Lower-case, accent-free, alphanumerics only ('RØDE NTH-100' -> 'rodenth100')."""
    out = []
    for c in unicodedata.normalize('NFKD', s.lower()):
        if unicodedata.combining(c):
            continue
        c = FOLD_EXTRA.get(c, c)
        if c.isalnum():
            out.append(c)
    return ''.join(out)


def split_variant(name):
    """'Beyerdynamic DT 770 Pro (worn earpads)' -> ('Beyerdynamic DT 770 Pro', ['worn earpads']).

    Only trailing parenthesised groups are variants ('Tansio Mirai (TSMR) X ...'
    keeps its inner group). Tolerates the few unbalanced names in AutoEq.
    """
    base, parts = name.strip(), []
    while base.endswith(')'):
        depth, i = 0, len(base) - 1
        while i >= 0:
            if base[i] == ')':
                depth += 1
            elif base[i] == '(':
                depth -= 1
                if depth == 0:
                    break
            i -= 1
        if i < 0:  # unmatched ')': drop it and look again
            base = base[:-1].rstrip()
            continue
        inner = base[i + 1:-1].strip()
        if inner:
            parts.insert(0, inner)
        base = base[:i].rstrip()
    if '(' in base and ')' not in base[base.rindex('('):]:  # unclosed '(' at the end
        i = base.rindex('(')
        inner = base[i + 1:].strip()
        if inner:
            parts.insert(0, inner)
        base = base[:i].rstrip()
    return base, parts


def pretty_variant(parts):
    out = []
    for p in parts:
        p = re.sub(r'\b[Ee]ar ?pads\b', 'pads', p)
        p = re.sub(r'\b[Ee]ar ?tips\b', 'tips', p)
        first = p.split(' ', 1)[0]
        if first.isalpha() and first.islower():
            p = p[0].upper() + p[1:]
        out.append(p)
    return ', '.join(out) if out else None


def find_manufacturer(words, manufacturers):
    """AutoEq's ManufacturerIndex.find: longest leading run of words that is a
    known manufacturer name or alias (case-insensitive) -> (true name, words used)."""
    for n in range(len(words), 0, -1):
        cand = ' '.join(words[:n]).lower()
        if cand in manufacturers:
            return manufacturers[cand], n
    return None, 0


def split_brand(base, manufacturers):
    """Brand / model with AutoEq's manufacturer index. Collaborations keep both
    names in the brand: 'Moondrop x Crinacle Blessing2 Dusk' -> 'Moondrop x Crinacle', 'Blessing2 Dusk'."""
    words = base.split(' ')
    brand, n = find_manufacturer(words, manufacturers)
    if brand is None:
        brand, n = words[0], 1
    rest = words[n:]
    if len(rest) >= 3 and rest[0] in ('x', 'X', '+', '&'):
        partner, k = find_manufacturer(rest[1:-1], manufacturers)
        k = k or 1
        brand = ' '.join([brand, rest[0]] + rest[1:1 + k])
        rest = rest[1 + k:]
    model = ' '.join(rest).strip()
    return brand, (model or base)


# ----------------------------------------------------------------- curves --

def read_curve(path):
    pts = []
    with open(path, encoding='utf-8', newline='') as fh:
        for row in csv.reader(fh):
            if len(row) < 2:
                continue
            try:
                f, v = float(row[0]), float(row[1])
            except ValueError:
                continue
            if f > 0 and math.isfinite(f) and math.isfinite(v):
                pts.append((f, v))
    pts.sort()
    dedup = []
    for f, v in pts:
        if not dedup or dedup[-1][0] != f:
            dedup.append((f, v))
    if len(dedup) < 10:
        raise ValueError(f'{path}: not a frequency response')
    return dedup


def resample(pts, grid):
    """Linear interpolation in log-frequency, clamped at the ends (= Curve::at)."""
    fs = [math.log(f) for f, _ in pts]
    vs = [v for _, v in pts]
    out, j, n = [], 0, len(pts)
    for f in grid:
        lf = math.log(f)
        if lf <= fs[0]:
            out.append(vs[0])
            continue
        if lf >= fs[-1]:
            out.append(vs[-1])
            continue
        while j + 1 < n and fs[j + 1] <= lf:
            j += 1
        t = (lf - fs[j]) / (fs[j + 1] - fs[j])
        out.append(vs[j] + t * (vs[j + 1] - vs[j]))
    return out


def centi(values, grid):
    band = [v for f, v in zip(grid, values) if ALIGN[0] <= f <= ALIGN[1]]
    mean = sum(band) / len(band)
    return [int(math.floor((v - mean) * 100.0 + 0.5)) for v in values]


def encode_curve(cdb):
    out = bytearray()
    prev = 0
    for v in cdb:
        d = v - prev
        prev = v
        z = (d << 1) ^ (d >> 63)  # zigzag
        while True:
            b = z & 0x7F
            z >>= 7
            if z:
                out.append(b | 0x80)
            else:
                out.append(b)
                break
    return bytes(out)


# ------------------------------------------------------------------ build --

def main():
    root = Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('autoeq', type=Path, help='AutoEq checkout (needs measurements/, targets/, dbtools/)')
    ap.add_argument('--out', type=Path, default=root / 'data' / 'catalog.bin')
    ap.add_argument('--md', type=Path, default=root / 'data' / 'catalog-sources.md')
    ap.add_argument('--per-octave', type=float, default=24.0, help='grid resolution (points per octave)')
    args = ap.parse_args()
    repo = args.autoeq.resolve()

    ppo = args.per_octave
    n_points = int(math.ceil(math.log2(F_TOP / F0) * ppo)) + 1
    grid = [F0 * 2.0 ** (i / ppo) for i in range(n_points)]

    webapp_targets = read_webapp_targets(repo)
    priorities = read_priorities(repo)
    manufacturers = read_manufacturers(repo)

    # (source, form, rig or None) -> target dict; first recommendation wins, like
    # ApiClient.fetchTargets in the web app.
    recommended = {}
    for t in webapp_targets:
        for m in t.get('recommended', []):
            key = (m['source'], m['form'], m.get('rig'))
            recommended.setdefault(key, t)

    # Priority rank per form: 0 = most trusted. Keys are (source, '<rig> <form>' or '<form>').
    rank = {}
    per_form = Counter()
    for source, form_rig in priorities:
        form = next(f for f in FORMS if form_rig.endswith(f))
        rank[(source, form_rig)] = per_form[form]
        per_form[form] += 1

    meas_root = repo / 'measurements'
    canonical = {}  # lower-case source dir -> canonical name (merges 'rtings' into 'Rtings' on Linux)
    for t in webapp_targets:
        for m in t.get('recommended', []):
            canonical.setdefault(m['source'].lower(), m['source'])

    # ---- collect measurements
    found = []  # (source, form, rig, rig_in_key, name, path)
    for src_dir in sorted(p for p in meas_root.iterdir() if p.is_dir()):
        source = canonical.get(src_dir.name.lower(), src_dir.name)
        data = src_dir / 'data'
        if not data.is_dir():
            continue
        name_index = read_name_index(src_dir / 'name_index.tsv')
        for form in FORMS:
            fdir = data / form
            if not fdir.is_dir():
                continue
            for item in sorted(fdir.iterdir()):
                if item.is_dir():
                    for f in sorted(item.glob('*.csv')):
                        found.append((source, form, clean(item.name), True, clean(f.stem), f))
                elif item.suffix == '.csv':
                    name = clean(item.stem)
                    rig = CRAWLER_RIGS.get(source)
                    if rig is None:
                        rows = name_index.get(name, [])
                        rigs = [r for fm, r in rows if fm == form and r] or [r for _, r in rows if r]
                        rig = rigs[0] if rigs else None
                    if rig:
                        rig = re.sub(r'\b(\w+) \1\b', r'\1', rig)  # 'GRAS GRAS 43AC'
                    found.append((source, form, rig, False, name, item))

    # ---- keys must be unique: the same source + name filed under two forms is a
    # mislabelled channel in a name index; keep the form most sources agree on.
    form_votes = defaultdict(Counter)
    for source, form, rig, rig_in_key, name, path in found:
        form_votes[name][form] += 1
    by_key = defaultdict(list)
    for item in found:
        source, form, rig, rig_in_key, name, path = item
        by_key[f'{source}/{rig}/{name}' if rig_in_key else f'{source}/{name}'].append(item)
    skipped = Counter()
    kept = []
    for key, items in by_key.items():
        if len(items) > 1:
            items.sort(key=lambda it: (-form_votes[it[4]][it[1]], FORM_CODE[it[1]]))
            for it in items[1:]:
                log(f'warning: duplicate key {key!r}: keeping {items[0][1]}, dropping {it[1]}')
                skipped[('duplicate', it[1], key)] += 1
        kept.append((key, items[0]))

    # ---- entries and the targets they need
    target_order = []
    target_index = {}
    entries = []
    for key, (source, form, rig, rig_in_key, name, path) in kept:
        t = recommended.get((source, form, rig if rig_in_key else None))
        if t is None:
            skipped[(source, form, rig if rig_in_key else None)] += 1
            continue
        if t['file'] not in target_index:
            target_index[t['file']] = len(target_order)
            target_order.append(t)
        form_rig = f'{rig} {form}' if rig_in_key else form
        r = rank.get((source, form_rig))
        if r is None:
            log(f'warning: no AutoEq priority for {source} / {form_rig}')
            r = 255
        base, parts = split_variant(name)
        brand, model = split_brand(base, manufacturers)
        entries.append({
            'name': name, 'brand': brand, 'model': model, 'variant': pretty_variant(parts),
            'source': source, 'rig': rig, 'rig_in_key': rig_in_key, 'form': form,
            'rank': min(r, 255), 'target': target_index[t['file']], 'path': path,
        })

    # Default order (what an empty search lists, and the tie-break of every
    # search): model groups (same brand + model once folded, so "DT770 Pro" and
    # "DT 770 PRO" group) by best source rank, then popularity (number of
    # sources / rigs that measured it, then number of measurements), shorter
    # model, model, brand; inside a group by source rank, stock before
    # variants, variant, source. Entries are stored in this order.
    groups = defaultdict(list)
    for e in entries:
        e['_brand_c'], e['_model_c'] = fold(e['brand']), fold(e['model'])
        groups[(e['_brand_c'], e['_model_c'])].append(e)
    for members in groups.values():
        best = min(m['rank'] for m in members)
        stock = any(m['variant'] is None for m in members)
        n_sources = len({(m['source'], m['rig']) for m in members})
        for m in members:
            m['_group'] = (best, -n_sources, -len(members))
            m['_group_has_stock'] = stock
    entries.sort(key=lambda e: (e['_group'], len(e['_model_c']), e['_model_c'], e['_brand_c'], e['rank'],
                                e['variant'] is not None, e['variant'] or '', e['source'], e['rig'] or '', e['name']))

    # ---- strings
    strings, string_id = [], {}

    def sid(s):
        if s is None:
            return NONE
        if s not in string_id:
            string_id[s] = len(strings)
            strings.append(s)
        return string_id[s]

    provenance = autoeq_revision(repo)
    provenance_id = sid(provenance)

    # ---- curves
    curve_data = bytearray()
    blobs = {}

    def add_curve(cdb):
        blob = encode_curve(cdb)
        if blob in blobs:  # identical measurements share their bytes
            return blobs[blob]
        off = len(curve_data)
        curve_data.extend(blob)
        blobs[blob] = off
        return off

    target_records = []
    for t in target_order:
        cdb = centi(resample(read_curve(repo / 'targets' / f"{t['file']}.csv"), grid), grid)
        label = t.get('label') or t['file']
        label = re.sub(r' without bass$', '', label)
        bass = float(t['bassBoost']['gain'])
        target_records.append((sid(t['file']), sid(label), int(round(bass * 100)), add_curve(cdb)))
        t['_label'], t['_bass'] = label, bass

    entry_records = []
    for e in entries:
        cdb = centi(resample(read_curve(e['path']), grid), grid)
        flags = (1 if e['rig_in_key'] else 0) | (2 if e['_group_has_stock'] else 0)
        entry_records.append(struct.pack(
            '<6I4BI', sid(e['name']), sid(e['brand']), sid(e['model']), sid(e['variant']),
            sid(e['source']), sid(e['rig']), FORM_CODE[e['form']], e['rank'], flags, e['target'],
            add_curve(cdb)))

    # ---- write blob
    encoded = [s.encode('utf-8') for s in strings]
    offsets, pos = [], 0
    for b in encoded:
        offsets.append(pos)
        pos += len(b)
    offsets.append(pos)
    out = io.BytesIO()
    out.write(struct.pack('<4sHHffIIIIII', b'FLC1', 1, n_points, F0, ppo, len(strings), len(entry_records),
                          len(target_records), len(curve_data), provenance_id, 0))
    out.write(struct.pack(f'<{len(offsets)}I', *offsets))
    out.write(b''.join(encoded))
    for rec in entry_records:
        out.write(rec)
    for name_id, label_id, bass, off in target_records:
        out.write(struct.pack('<IIhHI', name_id, label_id, bass, 0, off))
    out.write(curve_data)
    blob = out.getvalue()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(blob)

    # ---- report
    write_markdown(args.md, entries, target_order, recommended, rank, skipped, provenance, n_points, ppo, len(blob))
    log(f'{args.out}: {len(blob):,} bytes, {len(entries)} entries, {len(target_order)} targets, '
        f'{len(strings)} strings, {n_points} points at 1/{ppo:g} octave')
    for k, n in sorted(skipped.items(), key=str):
        log(f'skipped {n} x {k}')


def write_markdown(path, entries, targets, recommended, rank, skipped, provenance, n_points, ppo, size):
    by_group = OrderedDict()
    for e in sorted(entries, key=lambda e: (e['source'].lower(), FORM_CODE[e['form']], e['rig'] or '')):
        key = (e['source'], e['form'], e['rig'] if e['rig_in_key'] else None)
        g = by_group.setdefault(key, {'n': 0, 'rigs': Counter(), 'target': targets[e['target']], 'rank': e['rank']})
        g['n'] += 1
        if e['rig']:
            g['rigs'][e['rig']] += 1
    forms = Counter(e['form'] for e in entries)
    models = len({(e['brand'].lower(), e['model'].lower()) for e in entries})
    names = len({e['name'] for e in entries})
    sources = sorted({e['source'] for e in entries}, key=str.lower)

    L = []
    L.append('# Headphone catalog: sources')
    L.append('')
    L.append(f'Generated by `scripts/build_catalog.py` from {provenance}. Do not edit by hand.')
    L.append('')
    L.append(f'- **{len(entries):,} measurements** of **{names:,} distinct AutoEq names** '
             f'({models:,} brand + model groups, variants such as pads or ANC modes folded together), '
             f'from {len(sources)} sources.')
    L.append(f'- Over-ear {forms["over-ear"]:,} · in-ear {forms["in-ear"]:,} · earbuds {forms["earbud"]:,}.')
    L.append(f'- Curves: {n_points} points, 1/{ppo:g} octave from 20 Hz, centi-dB, delta + varint coded. '
             f'`data/catalog.bin` is {size:,} bytes.')
    L.append('')
    L.append('## Targets and default bass shelf')
    L.append('')
    L.append('Every measurement gets the target AutoEq\'s web app recommends for its source, form and rig, and that '
             'target\'s default bass shelf (low shelf at 105 Hz, Q 0.7). Both come from '
             '`dbtools/create_webapp_data.py` (`recommended`, `bassBoost`), which `ApiClient.fetchTargets` in '
             'autoeq.app uses to pick the target when a headphone is selected. The same gains are in '
             '`autoeq/constants.py` (`DEFAULT_BASS_BOOST_GAINS`).')
    L.append('')
    L.append('| Target (AutoEq file) | Label | Bass shelf | Used for |')
    L.append('|---|---|---:|---|')
    for t in targets:
        used = [f'{s} {f}' + (f' ({r})' if r else '') for (s, f, r), tt in recommended.items()
                if tt is t and any((s, f, r) == k for k in by_group)]
        L.append(f'| `{t["file"]}` | {t["_label"]} | +{t["_bass"]:g} dB | {", ".join(used)} |')
    L.append('')
    L.append('Cross-check against AutoEq\'s precomputed `results/` (target column of one result per source folder, '
             'fitted against every target and bass gain): the over-ear and in-ear folders match this table, e.g. '
             'oratory1990 Beyerdynamic DT 770 Pro = `Harman over-ear 2018 without bass` + 6.0 dB, except '
             'HypetheSonics over-ear (computed with about +10 dB on LMG 5128). Earbud results are inconsistent '
             'there (several sources were batch-processed with 0 dB of bass, Regan Cipher earbuds with the Harman '
             'over-ear target). The web-app table above is what autoeq.app uses today and is what this catalog '
             'follows. Flatline\'s own "Harman" preset (+6.5 dB) is half a dB above AutoEq\'s +6 dB.')
    L.append('')
    L.append('## Sources')
    L.append('')
    L.append('Rank is AutoEq\'s own source priority for that form (`ResultPath.priorities` in '
             '`dbtools/update_result_indexes.py`, 0 = most trusted). Search results list the same headphone from '
             'several sources in this order: oratory1990 first, then the GRAS / KEMAR-pinna rigs, then the rest.')
    L.append('')
    L.append('| Source | Form | Rig | Rank | Entries | Target |')
    L.append('|---|---|---|---:|---:|---|')
    for (source, form, rig), g in sorted(by_group.items(), key=lambda kv: (FORM_CODE[kv[0][1]], kv[1]['rank'])):
        rigs = rig or ', '.join(r for r, _ in g['rigs'].most_common(3)) or '-'
        L.append(f'| {source} | {form} | {rigs} | {g["rank"]} | {g["n"]:,} | {g["target"]["_label"]} |')
    L.append('')
    if skipped:
        L.append('## Left out')
        L.append('')
        for k, n in sorted(skipped.items(), key=str):
            if k[0] == 'duplicate':
                L.append(f'- `{k[2]}` filed as {k[1]}: same source and name also filed under another form (a '
                         f'mislabelled channel in the source\'s name index); the form most sources use is kept.')
            else:
                s, f, r = k
                L.append(f'- {s} {f}{" (" + r + ")" if r else ""}: {n} measurement(s), no target recommended by '
                         f'AutoEq for this group (AutoEq computes no result for it either).')
        L.append('- crinacle: AutoEq no longer ships crinacle\'s raw measurements (`measurements/crinacle` only '
                 'keeps its name index), so none are included.')
        L.append('')
    L.append('## Licence and credits')
    L.append('')
    L.append('- Measurement data, target curves and the source/target mapping come from the '
             '[AutoEq](https://github.com/jaakkopasanen/AutoEq) project by Jaakko Pasanen (MIT licence).')
    L.append('- The measurements remain the work and property of their authors: oratory1990, Rtings, '
             'Innerfidelity (Tyll Hertsens, Stereophile), the legacy headphone.com database, and the community '
             'measurers ' + ', '.join(s for s in sources if s not in
                                      ('oratory1990', 'Rtings', 'Innerfidelity', 'Headphone.com Legacy')) + '. '
             'They are included with credit for headphone calibration; each catalog row names its source.')
    L.append('- Target curves are AutoEq\'s per-rig versions (`targets/`); Harman target research by Sean Olive, '
             'Todd Welti and colleagues.')
    L.append('')
    path.write_text('\n'.join(L), encoding='utf-8')


if __name__ == '__main__':
    main()
