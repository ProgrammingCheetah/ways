#!/usr/bin/env python3
"""Token burn from Claude Code transcripts, per run and per role.

Reads every *.jsonl under ~/.claude/projects (or --root) modified in the last
--hours, deduplicates assistant messages by id, and prints per-model totals,
a per-run table (main sessions and subagents) and the growth profile that the
framework's budget rules are written against (turn-one context, final context,
turns). Dollar figures are API list prices used as a proxy for limit weight.
Stands in for `zaf report` until that verb exists.

Usage: tools/burn.py [--hours 168] [--root DIR] [--match SUBSTR] [--top 25]
"""
import argparse, collections, glob, json, os, re, statistics, time

RATES = {  # $/M: input, cache write, cache read, output (list prices, 2026-10)
    'claude-opus-5-5': (4, 5, 0.2, 20), 'claude-sonnet-5-5': (2, 2.5, 0.2, 10),
    'claude-fable-5-1': (10, 12.5, 0.25, 50), 'claude-haiku-5-5': (0.1, 0.125, 0.01, 0.5)}
DEFAULT = (4, 5, 0.2, 20)

def read_run(path):
    seen, ctx, first, models = set(), [], None, collections.Counter()
    tot = dict(n=0, cw=0, cr=0, out=0, cost=0.0)
    with open(path, errors='ignore') as fh:
        for line in fh:
            try: o = json.loads(line)
            except ValueError: continue
            t = o.get('type')
            if t == 'user' and first is None:
                c = o.get('message', {}).get('content')
                if isinstance(c, list): c = ' '.join(x.get('text', '') for x in c if isinstance(x, dict))
                if isinstance(c, str) and c.strip() and not c.startswith('<'): first = re.sub(r'\s+', ' ', c)[:120]
            if t != 'assistant': continue
            m = o.get('message', {}); u = m.get('usage'); mid = m.get('id'); model = m.get('model')
            if not u or not model or mid in seen: continue
            seen.add(mid); models[model] += 1
            i, cw, cr, out = (u.get(k, 0) for k in ('input_tokens', 'cache_creation_input_tokens', 'cache_read_input_tokens', 'output_tokens'))
            r = RATES.get(model, DEFAULT)
            tot['n'] += 1; tot['cw'] += cw; tot['cr'] += cr; tot['out'] += out
            tot['cost'] += (i * r[0] + cw * r[1] + cr * r[2] + out * r[3]) / 1e6
            ctx.append(i + cw + cr)
    if not tot['n']: return None
    return dict(path=path, first=first or '', model=models.most_common(1)[0][0], ctx=ctx, **tot)

def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--hours', type=float, default=168); ap.add_argument('--root', default=os.path.expanduser('~/.claude/projects'))
    ap.add_argument('--match', default='', help='only runs whose path or first prompt contains this'); ap.add_argument('--top', type=int, default=25)
    a = ap.parse_args(); cut = time.time() - a.hours * 3600
    runs = [r for p in glob.glob(a.root + '/**/*.jsonl', recursive=True) if os.path.getmtime(p) >= cut
            for r in [read_run(p)] if r and (a.match in r['path'] or a.match in r['first'])]
    if not runs: print('no runs'); return
    per = collections.defaultdict(lambda: [0, 0, 0, 0, 0.0])
    for r in runs:
        v = per[r['model']]; v[0] += r['n']; v[1] += r['cw']; v[2] += r['cr']; v[3] += r['out']; v[4] += r['cost']
    print(f"runs {len(runs)}  turns {sum(r['n'] for r in runs)}  est ${sum(r['cost'] for r in runs):.0f}")
    print(f"{'model':18s} {'turns':>6s} {'cache write':>12s} {'cache read':>12s} {'output':>8s} {'est $':>7s}")
    for m, v in sorted(per.items(), key=lambda kv: -kv[1][4]):
        print(f"{m:18s} {v[0]:6d} {v[1]/1e6:11.1f}M {v[2]/1e6:11.1f}M {v[3]/1e6:7.2f}M {v[4]:7.0f}")
    subs = [r for r in runs if '/subagents/' in r['path'] and len(r['ctx']) >= 3]
    if subs:
        f = [r['ctx'][0] for r in subs]; l = [r['ctx'][-1] for r in subs]; n = [r['n'] for r in subs]
        fixed = sum(a * b for a, b in zip(f, n)); total = sum(sum(r['ctx']) for r in subs) or 1
        print(f"\nsubagent runs {len(subs)}: median {statistics.median(n):.0f} turns, context {statistics.median(f)/1e3:.0f}k at turn 1 -> {statistics.median(l)/1e3:.0f}k at the end; fixed part {100*fixed/total:.0f}% of context reads, growth {100-100*fixed/total:.0f}%")
    print(f"\n{'model':10s} {'est $':>6s} {'turns':>5s} {'ctx1':>6s} {'ctxN':>6s} {'out':>6s}  run")
    for r in sorted(runs, key=lambda r: -r['cost'])[:a.top]:
        kind = 'sub' if '/subagents/' in r['path'] else 'main'
        print(f"{r['model'][7:17]:10s} {r['cost']:6.1f} {r['n']:5d} {r['ctx'][0]/1e3:5.0f}k {r['ctx'][-1]/1e3:5.0f}k {r['out']/1e3:5.0f}k  {kind}: {r['first'][:70]}")

if __name__ == '__main__': main()
