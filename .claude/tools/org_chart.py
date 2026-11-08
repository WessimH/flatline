#!/usr/bin/env python3
"""Draws the team org chart as an SVG for the GitBook docs.

Reads the identity line ("Tu t'appelles **First Last**, *Title* (Dept)")
of every agent in .claude/agents/ and the REPORTS_TO table below, then writes
docs/.gitbook/assets/team/organigramme.svg.
Run from the repository root: python3 .claude/tools/org_chart.py
"""
import glob, re, html

# Who reports to whom (agent name -> manager agent name, "wessim" = founder).
REPORTS_TO = {
    'po': 'wessim', 'hr': 'wessim',
    'ba': 'po', 'architect': 'po', 'challenger': 'po', 'ux-ui': 'po',
    'marketing': 'po', 'release': 'po', 'doc': 'po',
    'sound-engineer': 'architect', 'dev': 'architect', 'web-dev': 'architect',
    'qa': 'architect', 'devops': 'architect', 'minimalist': 'architect',
    'graphic-designer': 'ux-ui', 'com': 'marketing',
}
DEPT_COLOR = {'Product': '#FF2E4D', 'Engineering': '#FF7A5C', 'Review': '#C9A0FF',
              'Design': '#FFC56E', 'Go-to-market': '#6FD3C1', 'People': '#8FB8FF'}

people = {'wessim': ('Wessim Harmel', 'Founder', 'Founder')}
for path in glob.glob('.claude/agents/*.md'):
    key = path.split('/')[-1][:-3]
    m = re.search(r"Tu t'appelles \*\*(.+?)\*\*, \*(.+?)\* \((.+?)\)", open(path).read())
    if m:
        people[key] = m.groups()

def kids(k):
    return sorted([c for c, b in REPORTS_TO.items() if b == k and c in people], key=lambda c: people[c][2])

W, H, GX, GY = 168, 62, 14, 70
pos, nxt = {}, [0]
def place(k, depth):
    ch = kids(k)
    for c in ch:
        place(c, depth + 1)
    if ch:
        x = (pos[ch[0]][0] + pos[ch[-1]][0]) / 2
    else:
        x = nxt[0]
        nxt[0] += W + GX
    pos[k] = (x, depth)
place('wessim', 0)

width = nxt[0] + 40
depth = max(d for _, d in pos.values()) + 1
height = depth * (H + GY) + 40
out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" font-family="Helvetica, Arial, sans-serif">',
       f'<rect width="{width}" height="{height}" rx="16" fill="#07060A"/>']
xy = lambda k: (20 + pos[k][0], 20 + pos[k][1] * (H + GY))
for c, b in REPORTS_TO.items():
    if c in pos and b in pos:
        (x1, y1), (x2, y2) = xy(b), xy(c)
        ym = y1 + H + GY / 2
        out.append(f'<path d="M{x1+W/2} {y1+H} V{ym} H{x2+W/2} V{y2}" stroke="#F4EFEA" stroke-opacity="0.35" fill="none"/>')
for k in pos:
    x, y = xy(k)
    name, title, dept = people[k]
    col = DEPT_COLOR.get(dept, '#F4EFEA')
    out.append(f'<rect x="{x}" y="{y}" width="{W}" height="{H}" rx="12" fill="#1A1620" stroke="{col}" stroke-width="1.5"/>')
    out.append(f'<rect x="{x}" y="{y+10}" width="4" height="{H-20}" rx="2" fill="{col}"/>')
    out.append(f'<text x="{x+14}" y="{y+24}" fill="#F4EFEA" font-size="13" font-weight="700">{html.escape(name)}</text>')
    t = title if len(title) <= 26 else title[:25] + '…'
    out.append(f'<text x="{x+14}" y="{y+42}" fill="#D9CFCB" font-size="11">{html.escape(t)}</text>')
    if k != 'wessim':
        out.append(f'<text x="{x+14}" y="{y+55}" fill="{col}" font-size="9.5">{html.escape(dept)} · {k}</text>')
out.append('</svg>')
open('docs/.gitbook/assets/team/organigramme.svg', 'w').write('\n'.join(out))
print('wrote docs/.gitbook/assets/team/organigramme.svg', width, 'x', height)
