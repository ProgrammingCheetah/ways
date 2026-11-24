#!/usr/bin/env python3
"""Tests of the reference optimiser: the fixtures, then properties on random plans.

Run: python3 -m unittest test_reference -v   (from this folder)

The fixtures are the contract for the Rust implementation: it loads the same files and must
give the same plans and measures. The properties are what must always hold, on random plans of
seven kinds (D8): plain; with the vocabulary; with needs other epics provide; one epic's plan, so
interface ids carry the epic; with names other epics need; filed with `Blocked by`; and a re-plan
beside tickets already filed.
"""
import copy
import glob
import json
import os
import random
import unittest

from optimize import Refused, baseline, meaning, names, nat, optimize, reach, rerun_input, size
from simulate import by_size, simulate

FIXTURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "fixtures")


def load_fixtures():
    paths = sorted(glob.glob(os.path.join(FIXTURES, "*.json")))
    out = []
    for path in paths:
        with open(path) as handle:
            out.append(json.load(handle))
    return out


class Fixtures(unittest.TestCase):
    def test_there_are_fixtures(self):
        self.assertGreaterEqual(len(load_fixtures()), 29)

    def test_every_fixture_gives_its_expected_result(self):
        for fixture in load_fixtures():
            with self.subTest(fixture["name"]):
                expect = fixture["expect"]
                options = fixture["options"]
                if "refused" in expect:
                    with self.assertRaises(Refused) as caught:
                        optimize(fixture["plan"], options["lift"], options["interfaces"])
                    self.assertEqual(caught.exception.code, expect["refused"])
                    continue
                plan, after = optimize(fixture["plan"], options["lift"], options["interfaces"])
                self.assertEqual(plan["nodes"], expect["plan"]["nodes"])
                self.assertEqual(plan["orders"], expect["plan"]["orders"])
                self.assertEqual(plan["kept"], expect["plan"].get("kept", []))
                for listed in ("no_signature", "existing"):
                    if listed in expect["plan"]:
                        self.assertEqual(plan[listed], expect["plan"][listed])
                self.assertEqual(after, expect["after"])
                self.assertEqual(baseline(fixture["plan"])[1], expect["before"])

    def test_a_rerun_of_every_fixture_changes_nothing_ids_included(self):
        # O3 and S4 (D9): one run reaches the end, so f(f(A)) = f(A), ids included.
        for fixture in load_fixtures():
            if "refused" in fixture["expect"]:
                continue
            with self.subTest(fixture["name"]):
                options = fixture["options"]
                first = optimize(fixture["plan"], options["lift"], options["interfaces"])
                again = optimize(rerun_input(first[0]), options["lift"], options["interfaces"])
                self.assertEqual(again, first)

    def test_a_kept_wait_on_a_name_with_a_signature_comes_from_an_interface(self):
        # O3 as signed (D13): another epic's signature is in its interface ticket.
        for fixture in load_fixtures():
            if "refused" in fixture["expect"]:
                continue
            with self.subTest(fixture["name"]):
                options = fixture["options"]
                out, _ = optimize(fixture["plan"], options["lift"], options["interfaces"])
                for wait in out["kept"]:
                    if wait["needs"] not in out["no_signature"]:
                        self.assertTrue(wait["from"].startswith("I"), wait)

    def test_listing_order_gives_the_same_plan(self):
        by_name = {f["name"]: f for f in load_fixtures()}
        self.assertEqual(
            by_name["12-update-yes-and-the-mcp"]["expect"],
            by_name["17-listing-order-does-not-matter"]["expect"],
        )


def random_plan(rng):
    """A plan that is never broken: a node only needs names from nodes made before it."""
    count = rng.randint(1, 14)
    areas = [f"area{i}" for i in range(rng.randint(1, 6))]
    provided = []
    nodes = []
    for index in range(count):
        behaviours = []
        for part in range(rng.choice([1, 1, 1, 2])):
            name = f"n{index}.{part}"
            needs = rng.sample(provided, k=min(len(provided), rng.choice([0, 0, 1, 1, 2, 3])))
            behaviours.append({
                "text": f"behaviour {index}.{part}",
                "size": rng.choice([1, 1, 2]),
                "writes": rng.sample(areas, k=rng.choice([0, 1, 1, 2]) if len(areas) > 1 else 1),
                "provides": [name],
                "needs": needs + (["old"] if rng.random() < 0.2 else []),
            })
        provided += [b["provides"][0] for b in behaviours]
        nodes.append({"id": str(100 + index), "behaviours": behaviours})
    rng.shuffle(nodes)
    return {
        "limit": 2,
        "lanes": rng.choice([2, 4]),
        "existing": ["old"],
        "no_signature": [name for name in provided if rng.random() < 0.25],
        "nodes": nodes,
    }


def provided_names(plan):
    return sorted({name for n in plan["nodes"] for b in n["behaviours"] for name in b["provides"]})


def with_vocabulary(rng, plan):
    """The lists move into a vocabulary; the plan adds a few names of its own."""
    plan = copy.deepcopy(plan)
    no_signature = set(plan.pop("no_signature"))
    existing = plan.pop("existing")
    names_ = provided_names(plan)
    added = {name for name in names_ if rng.random() < 0.2}
    areas = sorted({a for n in plan["nodes"] for b in n["behaviours"] for a in b["writes"]})
    plan["vocabulary"] = {
        "areas": areas,
        "signature": sorted(n for n in names_ if n not in no_signature and n not in added),
        "no_signature": sorted(n for n in names_ if n in no_signature and n not in added),
        "existing": existing,
    }
    plan["new_names"] = {
        "signature": sorted(n for n in added if n not in no_signature),
        "no_signature": sorted(n for n in added if n in no_signature),
    }
    return plan


def with_outside(rng, plan):
    """Some needs are other epics' names: one with a signature sits in that epic's interface
    ticket (D13), one with none in its task."""
    plan = copy.deepcopy(plan)
    plan["outside"] = {"ext.sig": "I@9.1", "ext.plain": "#901"}
    plan["no_signature"] = sorted(set(plan["no_signature"]) | {"ext.plain"})
    for node in plan["nodes"]:
        for b in node["behaviours"]:
            if rng.random() < 0.3:
                b["needs"] = sorted(set(b["needs"]) | {rng.choice(["ext.sig", "ext.plain"])})
    return plan


def in_one_epic(plan):
    plan = copy.deepcopy(plan)
    for node in plan["nodes"]:
        node["epic"] = "7"
    return plan


def with_shared(rng, plan):
    plan = copy.deepcopy(plan)
    plan["shared"] = [name for name in provided_names(plan) if rng.random() < 0.3]
    return plan


def filed_with_blocked_by(rng):
    """A plan filed before 0.6.0: tasks, `Blocked by` lines, one of them naming an epic."""
    count = rng.randint(1, 12)
    nodes = []
    for index in range(count):
        earlier = [str(100 + i) for i in range(index)]
        blockers = rng.sample(earlier, k=min(len(earlier), rng.choice([0, 1, 1, 2])))
        if rng.random() < 0.2:
            blockers.append("E9")
        nodes.append({
            "id": str(100 + index),
            "blocked_by": blockers,
            "behaviours": [{"text": f"task {index}", "size": rng.choice([1, 1, 2]), "writes": [], "provides": [], "needs": []}],
        })
    rng.shuffle(nodes)
    return {"filed_with": "blocked_by", "epics": {"E9": ["900", "901"]}, "nodes": nodes}


def replanned(rng, plan, lift_mode, interfaces):
    """The epic's first plan, optimised and filed (each ticket open or done), then a second plan
    beside it (D29). New tasks need new names, and filed names only where no interface is
    missing: an interface's, a name with no signature, or a done task's."""
    first, _ = optimize(in_one_epic(plan), lift_mode, interfaces)
    done = {n["id"] for n in first["nodes"] if rng.random() < 0.3}
    grew = True
    while grew:  # a task is done only once all it waits on is done
        before = len(done)
        done |= {o["from"] for o in first["orders"] if o["to"] in done}
        grew = len(done) != before
    filed = []
    for node in first["nodes"]:
        node = copy.deepcopy(node)
        node["filed"] = "done" if node["id"] in done else "open"
        node["turns"] = [
            [o["from"], o["needs"][5:]] for o in first["orders"]
            if o["to"] == node["id"] and o["needs"].startswith("turn:")
        ]
        filed.append(node)
    no_signature = set(first["no_signature"])
    usable = sorted(
        {name for n in filed for b in n["behaviours"] for name in b["provides"]
         if n["kind"] == "interface" or name in no_signature or n["filed"] == "done"}
    )
    areas = sorted({a for n in filed for b in n["behaviours"] for a in b["writes"] if not a.startswith("sig:")}) or ["area0"]
    new = []
    made = []
    for index in range(rng.randint(1, 6)):
        name = f"r{index}"
        pool = usable + made
        new.append({
            "id": f"R{index}",
            "epic": "7",
            "behaviours": [{
                "text": f"re-plan {index}",
                "size": rng.choice([1, 1, 2]),
                "writes": rng.sample(areas, k=1),
                "provides": [name],
                "needs": rng.sample(pool, k=min(len(pool), rng.choice([0, 1, 1, 2]))),
            }],
        })
        made.append(name)
    return {
        "limit": 2,
        "lanes": first["lanes"],
        "record": 2,
        "existing": first["existing"],
        "no_signature": sorted(no_signature | {name for name in made if rng.random() < 0.25}),
        "nodes": filed + new,
    }


KINDS = ("plain", "vocabulary", "outside", "one epic", "shared", "blocked by", "re-plan")


def live(plan):
    """What a plan builds still: its body nodes but the done ones."""
    return dict(plan, nodes=[n for n in plan["nodes"] if n.get("filed") != "done"])


class Properties(unittest.TestCase):
    ROUNDS = 400

    def plans(self, kinds=KINDS):
        """Each round one kind of plan, in the four option sets."""
        rng = random.Random(20261122)
        for round_ in range(self.ROUNDS):
            kind = KINDS[round_ % len(KINDS)]
            base = random_plan(rng)
            if kind not in kinds:
                continue
            for lift_mode in ("all", "none"):
                for interfaces in ("each", "one"):
                    if kind == "plain":
                        plan = base
                    elif kind == "vocabulary":
                        plan = with_vocabulary(random.Random(round_), base)
                    elif kind == "outside":
                        plan = with_outside(random.Random(round_), base)
                    elif kind == "one epic":
                        plan = in_one_epic(base)
                    elif kind == "shared":
                        plan = with_shared(random.Random(round_), in_one_epic(base))
                    elif kind == "blocked by":
                        plan = filed_with_blocked_by(random.Random(round_))
                    else:
                        plan = replanned(random.Random(round_), base, lift_mode, interfaces)
                    yield plan, lift_mode, interfaces

    def test_every_kind_of_plan_is_made(self):
        made = {kind: 0 for kind in KINDS}
        for kind in KINDS:
            made[kind] = sum(1 for _ in self.plans(kinds=(kind,)))
        self.assertTrue(all(count > 0 for count in made.values()), made)

    def test_the_meaning_is_kept(self):
        for plan, lift_mode, interfaces in self.plans():
            out, _ = optimize(plan, lift_mode, interfaces)
            self.assertEqual(meaning(out), meaning(live(plan)))

    def test_no_cycle_and_every_edge_has_both_ends(self):
        for plan, lift_mode, interfaces in self.plans():
            out, after = optimize(plan, lift_mode, interfaces)
            ids = {n["id"] for n in out["nodes"]}
            self.assertEqual(len(ids), len(out["nodes"]))
            for order in out["orders"]:
                self.assertIn(order["from"], ids)
                self.assertIn(order["to"], ids)
            self.assertEqual(sum(after["width"]), len(ids))  # every node got a layer: no cycle

    def test_no_body_node_passes_the_limit(self):
        for plan, lift_mode, interfaces in self.plans():
            out, _ = optimize(plan, lift_mode, interfaces)
            for node in out["nodes"]:
                if node["kind"] == "body":
                    self.assertLessEqual(size(node), out["limit"])

    def test_the_same_plan_gives_the_same_result_however_it_is_listed(self):
        rng = random.Random(7)
        for plan, lift_mode, interfaces in self.plans():
            first = optimize(plan, lift_mode, interfaces)
            shuffled = dict(plan, nodes=rng.sample(plan["nodes"], k=len(plan["nodes"])))
            self.assertEqual(first, optimize(shuffled, lift_mode, interfaces))

    def test_no_two_nodes_that_write_the_same_thing_are_left_unordered(self):
        for plan, lift_mode, interfaces in self.plans():
            out, _ = optimize(plan, lift_mode, interfaces)
            orders = {(o["from"], o["to"], o["needs"]) for o in out["orders"]}
            reached = reach(out, orders)
            nodes = out["nodes"]
            for i, a in enumerate(nodes):
                for b in nodes[i + 1:]:
                    if names(a, "writes") & names(b, "writes"):
                        self.assertTrue(b["id"] in reached[a["id"]] or a["id"] in reached[b["id"]])

    def test_no_wait_is_left_that_another_path_implies(self):
        for plan, lift_mode, interfaces in self.plans():
            out, _ = optimize(plan, lift_mode, interfaces)
            orders = {(o["from"], o["to"], o["needs"]) for o in out["orders"]}
            for edge in orders:
                rest = orders - {edge}
                self.assertNotIn(edge[1], reach(out, rest).get(edge[0], set()))

    def test_after_lifting_a_body_waits_on_a_body_only_without_a_signature(self):
        for plan, lift_mode, interfaces in self.plans():
            if lift_mode != "all":
                continue
            out, _ = optimize(plan, lift_mode, interfaces)
            kind = {n["id"]: n["kind"] for n in out["nodes"]}
            for order in out["orders"]:
                if kind[order["from"]] == "body":
                    name = order["needs"]
                    self.assertTrue(name.startswith("turn:") or name in out["no_signature"], order)

    def test_a_rerun_changes_nothing_ids_included(self):
        # O3 and S4 (D9): the optimiser run on its own output gives the same plan and measures.
        for plan, lift_mode, interfaces in self.plans():
            first = optimize(plan, lift_mode, interfaces)
            self.assertEqual(optimize(rerun_input(first[0]), lift_mode, interfaces), first)

    def test_the_critical_path_is_a_path_of_the_critical_length(self):
        # O8 (D15): its nodes follow the waits, first to last, and their sizes add up to it.
        for plan, lift_mode, interfaces in self.plans():
            out, after = optimize(plan, lift_mode, interfaces)
            path = after["critical_path"]
            by_id = {n["id"]: n for n in out["nodes"]}
            edges = {(o["from"], o["to"]) for o in out["orders"]}
            self.assertEqual(sum(size(by_id[i]) for i in path), after["critical"])
            for first, second in zip(path, path[1:]):
                self.assertIn((first, second), edges)
            self.assertFalse(any(to == path[0] for _, to in edges))

    def test_lifting_never_makes_the_chain_longer_than_the_plan_as_written_plus_one(self):
        # Lifting alone gives a chain of 2. Conflicts put in order are what can lengthen it.
        for plan, lift_mode, interfaces in self.plans(kinds=("plain",)):
            if lift_mode != "all" or plan["no_signature"]:
                continue
            quiet = dict(plan, nodes=[
                dict(n, behaviours=[dict(b, writes=[]) for b in n["behaviours"]]) for n in plan["nodes"]
            ])
            _, after = optimize(quiet, lift_mode, interfaces)
            self.assertLessEqual(after["chain"], 2)

    def test_a_kept_wait_on_a_name_with_a_signature_comes_from_an_interface(self):
        # O3 as signed (D13).
        for plan, lift_mode, interfaces in self.plans():
            out, _ = optimize(plan, lift_mode, interfaces)
            for wait in out["kept"]:
                if wait["needs"] not in out["no_signature"]:
                    self.assertTrue(wait["from"].startswith("I"), wait)

    def test_a_filed_ticket_is_never_changed(self):
        # D29: a re-plan adds; "A ticket is never expanded".
        for plan, lift_mode, interfaces in self.plans(kinds=("re-plan",)):
            out, _ = optimize(plan, lift_mode, interfaces)
            by_id = {n["id"]: n for n in out["nodes"]}
            for node in plan["nodes"]:
                if node.get("filed") == "open":
                    self.assertIn(node["id"], by_id)
                    self.assertEqual(by_id[node["id"]]["behaviours"], node["behaviours"])

    def test_the_simulator_finishes_every_plan_and_more_lanes_never_take_longer(self):
        minutes = by_size({1: 9.0, 2: 14.0}, interface_each=5.0)
        rng = random.Random(11)
        for _ in range(100):
            out, _ = optimize(random_plan(rng))
            one = simulate(out, minutes, lanes=1)
            many = simulate(out, minutes, lanes=64)
            self.assertEqual(len(one["landed_at"]), len(out["nodes"]))
            self.assertLessEqual(many["minutes"], one["minutes"])
            self.assertEqual(one["lane_minutes"], many["lane_minutes"])


class Ordering(unittest.TestCase):
    def test_ids_sort_as_numbers(self):
        self.assertEqual(sorted(["10", "9", "I:a", "9.b", "9.a"], key=nat), ["9", "9.a", "9.b", "10", "I:a"])


if __name__ == "__main__":
    unittest.main()
