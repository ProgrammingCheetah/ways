#!/usr/bin/env python3
"""Reference implementation of the plan optimiser (0.6.0 draft, 2026-11-22).

A plan is a list of nodes (tasks). Each node holds behaviours; each behaviour says what it
writes, provides and needs, by name. Order edges are derived: the provider of a name comes
before every node that needs it. The optimiser rewrites the plan without changing what it
builds: split, lift (an order edge becomes an interface), settle conflicts, thin, measure.

No model, no clock, no randomness: the same plan gives the same result, whatever order its
nodes are listed in. This file is the oracle for the fixtures beside it; the real one is Rust.

Amended by the 0.6.0 design (2026-11-23): ids survive a rerun (f(f(A)) = f(A)), the measure
names the critical path's nodes, the names are checked against the release's vocabulary, a need
another epic provides is kept and never optimised, a plan filed with `Blocked by` reads as
needs, and a re-plan adds to tickets already filed without changing them. Each amendment names
its decision in the design (D numbers).

Usage: optimize.py <plan.json> [--lift all|none] [--interfaces each|one]
"""
import copy
import json
import re
import sys


class Refused(Exception):
    """The plan is broken. The optimiser never repairs one."""

    def __init__(self, code, detail):
        super().__init__(f"{code}: {detail}")
        self.code = code
        self.detail = detail


def nat(text):
    """Sort key: digits compare as numbers, so 9 comes before 10."""
    return [int(part) if part.isdigit() else part for part in re.split(r"(\d+)", text)]


def size(node):
    return sum(b["size"] for b in node["behaviours"])


def names(node, field):
    return {name for b in node["behaviours"] for name in b[field]}


def normalise(plan):
    plan = copy.deepcopy(plan)
    plan.setdefault("limit", 2)
    plan.setdefault("lanes", 4)
    plan.setdefault("record", 1)
    plan["shared"] = sorted(set(plan.get("shared", [])))
    vocabulary = plan.get("vocabulary")
    if vocabulary is not None:
        # D7: the lists come from the vocabulary; a planner never writes them.
        if "no_signature" in plan or "existing" in plan:
            raise Refused("planner-list", "no_signature and existing come from the vocabulary")
        new = plan.get("new_names", {})
        extra = sorted(set(new) - {"areas", "signature", "no_signature"})
        if extra:
            raise Refused("planner-list", f"a plan may not add {extra[0]} names")
        plan["no_signature"] = sorted(set(vocabulary.get("no_signature", [])) | set(new.get("no_signature", [])))
        plan["existing"] = sorted(set(vocabulary.get("existing", [])))
    plan.setdefault("no_signature", [])
    plan.setdefault("existing", [])
    plan.setdefault("outside", {})
    # D29: a re-plan carries the epic's filed tickets. A done one is code by now: what it provides
    # is existing, and it leaves the plan. An open one stays, fixed: never split, merged or lifted.
    done = [n for n in plan["nodes"] if n.get("filed") == "done"]
    plan["done_names"] = sorted({name for n in done for b in n["behaviours"] for name in b.get("provides", [])})
    plan["existing"] = sorted(set(plan["existing"]) | set(plan["done_names"]))
    plan["nodes"] = [n for n in plan["nodes"] if n.get("filed") != "done"]
    for node in plan["nodes"]:
        node.setdefault("kind", "body")
        for b in node.get("behaviours", []):
            b.setdefault("size", 1)
            for field in ("writes", "provides", "needs"):
                b[field] = sorted(set(b.get(field, [])))
    read_blocked_by(plan)
    plan["nodes"].sort(key=lambda n: nat(n["id"]))
    return plan


def read_blocked_by(plan):
    """D4: a plan filed with `Blocked by` still reads. Each task provides its own name `#<id>`;
    each line becomes a need of the name it names, and a line naming a whole epic a need of each
    task in that epic (`epics` lists them). A name no task of the plan provides is another epic's
    task: kept, never optimised (D13). These names have no signature, so no wait is lifted.
    Read only for a plan marked `"filed_with": "blocked_by"`."""
    if plan.pop("filed_with", None) != "blocked_by":
        return
    members = plan.pop("epics", {})
    own = {node["id"] for node in plan["nodes"]}
    names_made = set()
    for node in plan["nodes"]:
        needed = set()
        for blocker in node.pop("blocked_by", []):
            for task in members.get(str(blocker), [str(blocker)]):
                if task != node["id"]:
                    needed.add("#" + task)
        first = node["behaviours"][0]
        first["provides"] = sorted(set(first["provides"]) | {"#" + node["id"]})
        first["needs"] = sorted(set(first["needs"]) | needed)
        names_made |= needed | {"#" + node["id"]}
        for name in needed:
            if name[1:] not in own:
                plan["outside"].setdefault(name, name)
    plan["no_signature"] = sorted(set(plan["no_signature"]) | names_made)


def fixed(node):
    """D29: a ticket filed by an earlier plan of the epic, still open."""
    return node.get("filed") == "open"


def check_names(plan):
    """D7: every name is the vocabulary's or one the plan adds; an area only from the vocabulary.
    A filed ticket's names were checked when it was filed."""
    vocabulary = plan.get("vocabulary")
    if vocabulary is None:
        return
    new = plan.get("new_names", {})
    areas = set(vocabulary.get("areas", [])) | set(new.get("areas", []))
    known = set(plan["outside"]) | set(new.get("signature", [])) | set(new.get("no_signature", []))
    known |= set(plan["existing"])
    for kind in ("signature", "no_signature", "existing"):
        known |= set(vocabulary.get(kind, []))
    for node in plan["nodes"]:
        if fixed(node):
            known |= names(node, "provides")
    for node in plan["nodes"]:
        if fixed(node):
            continue
        for b in node["behaviours"]:
            for name in b["writes"]:
                if name not in areas:
                    raise Refused("unknown-name", f"{node['id']} writes {name}")
            for field in ("provides", "needs"):
                for name in b[field]:
                    if name not in known:
                        raise Refused("unknown-name", f"{node['id']} {field} {name}")


def check(plan):
    """Everything wrong with a plan that can be seen without its edges."""
    seen = set()
    for node in plan["nodes"]:
        if node["id"] in seen:
            raise Refused("duplicate-id", node["id"])
        seen.add(node["id"])
        if not node.get("behaviours"):
            raise Refused("no-behaviour", node["id"])
        if "+" in node["id"] and len(node["id"].split("+")) != len(node["behaviours"]):
            # D9: a merged node holds one behaviour per part of its id.
            raise Refused("bad-id", node["id"])
        for b in node["behaviours"]:
            if not b.get("text", "").strip():
                raise Refused("no-behaviour", node["id"])
            if not isinstance(b["size"], int) or b["size"] < 1:
                raise Refused("bad-size", node["id"])
            if node["kind"] == "body" and b["size"] > plan["limit"]:
                raise Refused("too-large", node["id"])
            own = set(b["provides"]) & set(b["needs"])
            if own:
                raise Refused("needs-itself", f"{node['id']} {sorted(own)[0]}")
    check_names(plan)
    provider = {}
    for node in plan["nodes"]:
        for b in node["behaviours"]:
            for name in b["provides"]:
                if name in provider or name in plan["outside"] or name in plan["done_names"]:
                    raise Refused("two-providers", name)
                provider[name] = node["id"]
    existing = set(plan["existing"])
    for node in plan["nodes"]:
        for name in sorted(names(node, "needs")):
            if name not in provider and name not in existing and name not in plan["outside"]:
                raise Refused("unprovided", f"{node['id']} needs {name}")
    # D29: a new task may not wait on an open filed task for a name with a signature that no
    # interface holds: the filed ticket cannot be changed to implement one, and O3 would fail.
    by_id = {n["id"]: n for n in plan["nodes"]}
    skip = set(plan["no_signature"])
    for node in plan["nodes"]:
        if fixed(node):
            continue
        for name in sorted(names(node, "needs")):
            owner = by_id.get(provider.get(name))
            if owner is not None and fixed(owner) and owner["kind"] == "body" and name not in skip:
                raise Refused("fixed-provider", f"{node['id']} needs {name} from {owner['id']}")


def part_id(base, index):
    return f"{base}.{chr(ord('a') + index)}" if index < 26 else f"{base}.{index}"


def part_ids(node):
    """D9: the id of each behaviour once split. A merged node splits back into the nodes it was
    made of (`A+B` into `A` and `B`), so a rerun gives the same ids; any other node with several
    behaviours into `<id>.a`, `<id>.b`, ..."""
    parts = node["id"].split("+")
    if len(parts) == len(node["behaviours"]) > 1:
        return parts
    return [part_id(node["id"], index) for index in range(len(node["behaviours"]))]


def split(plan):
    """One behaviour per node. An interface node, and a filed ticket (D29), is left whole."""
    out = []
    for node in plan["nodes"]:
        if node["kind"] != "body" or len(node["behaviours"]) == 1 or fixed(node):
            out.append(node)
            continue
        for index, b in enumerate(node["behaviours"]):
            part = {k: v for k, v in node.items() if k != "behaviours"}
            part["id"] = part_ids(node)[index]
            part["behaviours"] = [b]
            out.append(part)
    plan["nodes"] = sorted(out, key=lambda n: nat(n["id"]))


def providers(plan):
    return {name: n["id"] for n in plan["nodes"] for name in names(n, "provides")}


def derive(plan):
    """The order edges: the provider of a name comes before each node that needs it."""
    provider = providers(plan)
    orders = set()
    ids = {n["id"] for n in plan["nodes"]}
    for node in plan["nodes"]:
        # D29: a filed ticket's `Turn:` lines stay its waits; one after a done ticket is over.
        for after, area in node.get("turns", []):
            if after in ids:
                orders.add((after, node["id"], "turn:" + area))
        for name in names(node, "needs"):
            if name not in provider:  # existing code, or another epic's task (D13)
                continue
            if provider[name] != node["id"]:
                orders.add((provider[name], node["id"], name))
    return orders


def successors(orders):
    out = {}
    for a, b, _ in orders:
        out.setdefault(a, set()).add(b)
    return out


def reach(plan, orders):
    """For each node, every node that can be reached from it along order edges."""
    succ = successors(orders)
    memo = {}

    def visit(node_id):
        if node_id not in memo:
            memo[node_id] = set()
            found = set()
            for nxt in succ.get(node_id, ()):
                found.add(nxt)
                found |= visit(nxt)
            memo[node_id] = found
        return memo[node_id]

    for node in plan["nodes"]:
        visit(node["id"])
    return memo


def layers(plan, orders):
    """Longest-path layer of each node; refuses a cycle."""
    preds = {n["id"]: set() for n in plan["nodes"]}
    for a, b, _ in orders:
        preds[b].add(a)
    layer = {}
    remaining = sorted(preds, key=nat)
    while remaining:
        ready = [i for i in remaining if all(p in layer for p in preds[i])]
        if not ready:
            raise Refused("cycle", " ".join(remaining))
        step = {i: 1 + max((layer[p] for p in preds[i]), default=-1) for i in ready}
        layer.update(step)
        remaining = [i for i in remaining if i not in layer]
    return layer


def interface_id(owner, record):
    """D11, D29: one interface node per epic and per plan of it: `I@<epic>.<record>`, so a re-plan's
    signatures never land in a ticket already filed. A plan with no epic (a fixture) uses `I`."""
    if owner.get("epic"):
        return f"I@{owner['epic']}.{record}"
    return "I" if record == 1 else f"I.{record}"


def lift(plan, orders, interfaces):
    """An order edge becomes an interface: users wait on the shape, the provider implements it.
    A name other epics need (`shared`, D13) is lifted too, though nothing in this plan needs it,
    so their waits go through this epic's interface. A filed ticket is never lifted (D29)."""
    by_id = {n["id"]: n for n in plan["nodes"]}
    provider = providers(plan)
    skip = set(plan["no_signature"])

    def liftable(name):
        owner = by_id.get(provider.get(name))
        return name not in skip and owner is not None and owner["kind"] == "body" and not fixed(owner)

    lifted = sorted({name for _, _, name in orders if liftable(name)} | {n for n in plan["shared"] if liftable(n)})
    for name in lifted:
        owner = by_id[provider[name]]
        if interfaces == "one":
            iface_id = interface_id(owner, plan["record"])
        else:
            iface_id = "I:" + name
        iface = by_id.get(iface_id)
        if iface is None:
            iface = {"id": iface_id, "kind": "interface", "behaviours": []}
            if interfaces == "one" and owner.get("epic"):
                iface["epic"] = owner["epic"]
            by_id[iface_id] = iface
            plan["nodes"].append(iface)
        iface["behaviours"].append(
            {"text": "the signature of " + name, "size": 1, "writes": ["sig:" + name],
             "provides": [name], "needs": []}
        )
        for b in owner["behaviours"]:
            if name in b["provides"]:
                b["provides"].remove(name)
                b["needs"] = sorted(set(b["needs"]) | {name})
        for edge in [e for e in orders if e[2] == name]:
            orders.discard(edge)
            orders.add((iface_id, edge[1], name))
        orders.add((iface_id, owner["id"], name))
    plan["nodes"].sort(key=lambda n: nat(n["id"]))


def settle(plan, orders):
    """Two unordered nodes that write the same thing are merged if they fit, else put in order.
    D29: two filed tickets were settled when filed; a filed ticket and a new one take turns, the
    filed one first, and are never merged."""
    while True:
        nodes = sorted(plan["nodes"], key=lambda n: nat(n["id"]))
        reached = reach(plan, orders)
        pair = None
        for i, a in enumerate(nodes):
            for b in nodes[i + 1:]:
                if fixed(a) and fixed(b):
                    continue
                shared = names(a, "writes") & names(b, "writes")
                if shared and b["id"] not in reached[a["id"]] and a["id"] not in reached[b["id"]]:
                    pair = (a, b, shared)
                    break
            if pair:
                break
        if pair is None:
            return
        a, b, shared = pair
        if fixed(a) or fixed(b):
            first, second = (a, b) if fixed(a) else (b, a)
            orders.add((first["id"], second["id"], "turn:" + sorted(shared)[0]))
        elif a["kind"] == b["kind"] == "body" and size(a) + size(b) <= plan["limit"]:
            merged = {k: v for k, v in a.items() if k != "behaviours"}
            merged["id"] = a["id"] + "+" + b["id"]
            merged["behaviours"] = a["behaviours"] + b["behaviours"]
            plan["nodes"] = [n for n in plan["nodes"] if n is not a and n is not b] + [merged]
            gone = {a["id"], b["id"]}
            for edge in [e for e in orders if e[0] in gone or e[1] in gone]:
                orders.discard(edge)
                src = merged["id"] if edge[0] in gone else edge[0]
                dst = merged["id"] if edge[1] in gone else edge[1]
                if src != dst:
                    orders.add((src, dst, edge[2]))
        else:
            first, second = sorted(
                (a, b), key=lambda n: (-len(reached[n["id"]]), nat(n["id"]))
            )
            orders.add((first["id"], second["id"], "turn:" + sorted(shared)[0]))


def edge_key(edge):
    return (nat(edge[0]), nat(edge[1]), edge[2])


def thin(plan, orders):
    """Drop every wait that another path already implies. The node keeps the name in its needs."""
    for edge in sorted(orders, key=edge_key):
        orders.discard(edge)
        if edge[1] not in reach(plan, orders).get(edge[0], set()):
            orders.add(edge)


def measure(plan, orders):
    layer = layers(plan, orders)
    by_id = {n["id"]: n for n in plan["nodes"]}
    chain = 1 + max(layer.values(), default=-1)
    width = [sum(1 for v in layer.values() if v == k) for k in range(chain)]
    preds = {i: set() for i in by_id}
    for a, b, _ in orders:
        preds[b].add(a)
    longest = {}
    for node_id in sorted(by_id, key=lambda i: layer[i]):
        longest[node_id] = size(by_id[node_id]) + max((longest[p] for p in preds[node_id]), default=0)
    return {
        "nodes": len(by_id),
        "interfaces": sum(1 for n in by_id.values() if n["kind"] == "interface"),
        "chain": chain,
        "width": width,
        "narrow_layers": [k for k, w in enumerate(width) if w < plan["lanes"]],
        "work": sum(size(n) for n in by_id.values()),
        "critical": max(longest.values(), default=0),
        "critical_path": critical_path(by_id, preds, longest),
    }


def critical_path(by_id, preds, longest):
    """D15: the heaviest chain's nodes, first to last, weighed by size. It ends at the node where
    the heaviest chain ends, the lowest id on a tie; each step back stays on that chain (a node it
    waits on whose own heaviest chain is shorter by exactly this node's size), the lowest id on a
    tie."""
    if not longest:
        return []
    top = max(longest.values())
    current = min((i for i in longest if longest[i] == top), key=nat)
    path = []
    while current is not None:
        path.append(current)
        want = longest[current] - size(by_id[current])
        options = [p for p in preds[current] if longest[p] == want]
        current = min(options, key=nat) if options else None
    return path[::-1]


def meaning(plan):
    """What the plan builds: every behaviour of a body node, in order."""
    return sorted(
        b["text"] for n in plan["nodes"] if n.get("kind", "body") == "body" for b in n["behaviours"]
    )


def kept(plan):
    """D13: each wait on another epic's task, kept as it is and never optimised."""
    edges = {
        (plan["outside"][name], n["id"], name)
        for n in plan["nodes"] for name in names(n, "needs") if name in plan["outside"]
    }
    return [{"from": a, "to": b, "needs": name} for a, b, name in sorted(edges, key=edge_key)]


def as_plan(plan, orders):
    out = {k: plan[k] for k in ("limit", "lanes", "record", "shared", "no_signature", "existing", "outside")}
    out["nodes"] = sorted(plan["nodes"], key=lambda n: nat(n["id"]))
    out["orders"] = [
        {"from": a, "to": b, "needs": name} for a, b, name in sorted(orders, key=edge_key)
    ]
    out["kept"] = kept(plan)
    return out


def rerun_input(out):
    """The optimiser's output as the next run's input: its nodes and lists, no edges (D9)."""
    return {k: v for k, v in out.items() if k not in ("orders", "kept")}


def baseline(plan):
    """The plan as written, checked and measured, with nothing rewritten."""
    plan = normalise(plan)
    check(plan)
    orders = derive(plan)
    layers(plan, orders)
    return as_plan(plan, orders), measure(plan, orders)


def optimize(plan, lift_mode="all", interfaces="each"):
    plan = normalise(plan)
    check(plan)
    split(plan)
    orders = derive(plan)
    layers(plan, orders)
    if lift_mode == "all":
        lift(plan, orders, interfaces)
    settle(plan, orders)
    thin(plan, orders)
    return as_plan(plan, orders), measure(plan, orders)


def main(argv):
    path = argv[1]
    lift_mode = argv[argv.index("--lift") + 1] if "--lift" in argv else "all"
    interfaces = argv[argv.index("--interfaces") + 1] if "--interfaces" in argv else "each"
    with open(path) as handle:
        plan = json.load(handle)
    plan = plan.get("plan", plan)
    try:
        before = baseline(plan)[1]
        out, after = optimize(plan, lift_mode, interfaces)
    except Refused as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        return 1
    json.dump({"before": before, "after": after, "plan": out}, sys.stdout, indent=1)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
