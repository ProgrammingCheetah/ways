#!/usr/bin/env python3
"""Replay a real plan through the optimiser and the build simulator (0.6.0 draft).

Usage: replay.py ../real/zaf-0.5.5.json

The run minutes and the wait after a run are tonight's measured means (2026-11-22, 40 merged
tasks): a task held a lane about 12 minutes, then waited about 38 minutes for CI and its merge.
The interface minutes are assumptions, since no interface task has ever been run.
"""
import json
import sys

from optimize import baseline, optimize
from simulate import by_size, simulate

RUN = {1: 11.3, 2: 12.8, 3: 12.8}  # minutes in a lane, by size (XS and S, then M)
CI_WAIT = 38.0                     # minutes between a run's end and its merge, in no lane
MERGE_GAP = 0.2                    # the least time between two merges seen tonight
INTERFACE_EACH = 6.0               # assumed: one small run writes one signature
INTERFACE_BASE, INTERFACE_PER_NAME = 8.0, 1.5  # assumed: one run writes k signatures


def scenario(plan, label, lift_mode, interfaces, without_signature):
    plan = dict(plan, no_signature=sorted(without_signature))
    if lift_mode is None:
        out, measure = baseline(plan)
    else:
        out, measure = optimize(plan, lift_mode, interfaces)
    minutes = by_size(RUN, INTERFACE_EACH, INTERFACE_BASE, INTERFACE_PER_NAME)
    rows = {}
    for lanes in (2, 4, 8):
        rows[lanes] = simulate(out, minutes, lanes=lanes, ci_wait=CI_WAIT, merge_gap=MERGE_GAP)
    return {"label": label, "measure": measure, "sim": rows}


def per_epic(plan, label, without_signature, through_interface):
    """Each epic optimised on its own, as zaf does it (D13): a need another epic provides is kept,
    never optimised. With `through_interface`, a kept wait on a name with a signature is on the
    providing epic's interface ticket, which lifts every name other epics need; without it, the
    wait is on the providing task itself."""
    from optimize import measure, nat
    no_signature = set(without_signature)
    epic_of = {n["id"]: n["epic"] for n in plan["nodes"]}
    provider = {name: n["id"] for n in plan["nodes"] for b in n["behaviours"] for name in b["provides"]}
    nodes, orders = [], set()
    for epic in sorted({n["epic"] for n in plan["nodes"]}, key=nat):
        own = [n for n in plan["nodes"] if n["epic"] == epic]
        here = {name for n in own for b in n["behaviours"] for name in b["provides"]}
        outside, shared = {}, set()
        for n in own:
            for b in n["behaviours"]:
                for name in b["needs"]:
                    if name in here:
                        continue
                    other = epic_of[provider[name]]
                    signed = through_interface and name not in no_signature
                    outside[name] = f"I@{other}.1" if signed else provider[name]
        for n in plan["nodes"]:
            if n["epic"] != epic:
                for b in n["behaviours"]:
                    shared |= {name for name in b["needs"] if name in here}
        part = {k: v for k, v in plan.items() if k != "nodes"}
        part.update(nodes=own, outside=outside, no_signature=sorted(no_signature),
                    shared=sorted(shared) if through_interface else [])
        out, _ = optimize(part, "all", "one")
        nodes += out["nodes"]
        orders |= {(o["from"], o["to"], o["needs"]) for o in out["orders"]}
        orders |= {(k["from"], k["to"], k["needs"]) for k in out["kept"]}
    ids = {n["id"] for n in nodes}
    missing = sorted({a for a, _, _ in orders if a not in ids})
    if missing:
        raise ValueError(f"kept waits on nodes no epic made: {missing}")
    whole = {"limit": plan["limit"], "lanes": plan["lanes"], "nodes": nodes,
             "orders": [{"from": a, "to": b, "needs": n} for a, b, n in sorted(orders)]}
    result = measure(whole, orders)
    minutes = by_size(RUN, INTERFACE_EACH, INTERFACE_BASE, INTERFACE_PER_NAME)
    rows = {lanes: simulate(whole, minutes, lanes=lanes, ci_wait=CI_WAIT, merge_gap=MERGE_GAP) for lanes in (2, 4, 8)}
    return {"label": label, "measure": result, "sim": rows}


def main(path):
    with open(path) as handle:
        plan = json.load(handle)
    groups = set(plan.get("groups_without_signature", []))
    plan = {k: v for k, v in plan.items() if k not in ("about", "groups_without_signature")}
    no_code = {
        name for n in plan["nodes"] if n.get("group") in groups for b in n["behaviours"] for name in b["provides"]
    }
    everything = {name for n in plan["nodes"] for b in n["behaviours"] for name in b["provides"]}
    results = [
        scenario(plan, "as planned", None, "each", everything),
        scenario(plan, "thinned only", "none", "each", everything),
        scenario(plan, "code edges lifted, an interface each", "all", "each", no_code),
        scenario(plan, "code edges lifted, one interface run per epic", "all", "one", no_code),
        scenario(plan, "every edge lifted (upper bound)", "all", "one", set()),
        per_epic(plan, "per epic, kept waits on the providing task", no_code, False),
        per_epic(plan, "per epic, kept waits on its interface (D13)", no_code, True),
    ]
    print(f"{'scenario':47} {'nodes':>5} {'chain':>5} {'widest':>6} {'work':>5}  hours at 2 / 4 / 8 lanes   lanes busy at 4")
    for r in results:
        m, s = r["measure"], r["sim"]
        print(
            f"{r['label']:47} {m['nodes']:>5} {m['chain']:>5} {max(m['width']):>6} {m['work']:>5}  "
            f"{s[2]['hours']:>5} / {s[4]['hours']:>5} / {s[8]['hours']:>5}        {s[4]['mean_lanes_busy']:>5}"
        )
    print()
    for r in results:
        print(f"{r['label']}: width by layer {r['measure']['width']}")
    return results


if __name__ == "__main__":
    main(sys.argv[1])
