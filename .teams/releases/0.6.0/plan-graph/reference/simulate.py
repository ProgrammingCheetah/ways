#!/usr/bin/env python3
"""A small clock-driven model of a zaf build, to compare plans (0.6.0 draft, 2026-11-22).

What it models, and nothing more:
- `lanes` model runs at once. A node holds a lane for its run minutes, then leaves it.
- After its run a node waits for CI, holding no lane, then merges.
- Merges land one at a time, `merge_gap` minutes apart at least (one merge per tick).
- A node may start once every node it waits on has merged. Ready nodes start in id order.

It does not model questions, failed checks beyond the average fix time folded into a run,
or a model's mistakes. It answers one thing: how long does this graph take on these lanes.
"""
import heapq

from optimize import nat, size


def simulate(plan, minutes, lanes=None, ci_wait=10.0, merge_gap=2.0):
    """`minutes(node)` gives a node's run time. Returns the build's shape in time."""
    lanes = lanes or plan["lanes"]
    nodes = {n["id"]: n for n in plan["nodes"]}
    waits = {i: set() for i in nodes}
    after = {i: set() for i in nodes}
    for order in plan["orders"]:
        waits[order["to"]].add(order["from"])
        after[order["from"]].add(order["to"])

    ready = sorted((i for i in nodes if not waits[i]), key=nat)
    events = []  # (time, sequence, kind, id); the sequence keeps ties in a fixed order
    sequence = 0
    now = 0.0
    free = lanes
    busy = 0.0
    last_merge = float("-inf")
    merged = set()
    landed_at = {}

    while ready or events:
        while ready and free:
            node_id = ready.pop(0)
            free -= 1
            run = minutes(nodes[node_id])
            busy += run
            sequence += 1
            heapq.heappush(events, (now + run, sequence, "ran", node_id))
        now, _, kind, node_id = heapq.heappop(events)
        if kind == "ran":
            free += 1
            at = max(now + ci_wait, last_merge + merge_gap)
            last_merge = at
            sequence += 1
            heapq.heappush(events, (at, sequence, "merged", node_id))
        else:
            merged.add(node_id)
            landed_at[node_id] = now
            newly = [i for i in after[node_id] if waits[i] <= merged and i not in landed_at]
            ready = sorted(set(ready) | set(newly), key=nat)

    if len(merged) != len(nodes):
        raise ValueError("the plan has a cycle: some nodes never became ready")
    return {
        "minutes": round(now, 1),
        "hours": round(now / 60, 2),
        "lane_minutes": round(busy, 1),
        "mean_lanes_busy": round(busy / now, 2) if now else 0.0,
        "landed_at": landed_at,
    }


def by_size(table, interface_each=None, interface_base=8.0, interface_per_name=1.5):
    """Run minutes from a size table. An interface node of k names takes base + k * per_name,
    or `interface_each` when it holds one name and that is given."""

    def minutes(node):
        if node.get("kind") == "interface":
            count = len(node["behaviours"])
            if count == 1 and interface_each is not None:
                return interface_each
            return interface_base + interface_per_name * count
        return sum(table[min(b["size"], max(table))] for b in node["behaviours"])

    return minutes


__all__ = ["simulate", "by_size", "size"]
