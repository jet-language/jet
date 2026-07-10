# Canvas M1 Acceptance Script

1. Run `jet dev main.jet --target=web` on the demo project and open `/canvas`.
   Expected: Canvas opens on the `run` graph with nonblank nodes and graph tabs.

2. Click the `summarize` graph tab.
   Expected: the graph changes to `summarize`, and the overview shows node and wire counts.

3. Right-click empty graph space and search `Branch`.
   Expected: the action menu shows Flow actions, and choosing `Branch` writes an `if true` block into source.

4. Open the `scratch` graph, drag from the `limit` value pin, search `square`, and choose `square`.
   Expected: Canvas writes `square(limit)` and shows both data and exec wiring on the new call.

5. Drag from the `limit` value pin again, search `abs`, and choose the Core `abs` action.
   Expected: Canvas writes a checked Core call such as `math.abs(limit)`.

6. Click the inline value on the `print(limit)` node, change it to `limit + 2`, and apply.
   Expected: source changes to `print(limit + 2)` and the graph reprojects without drift.

7. Open the Variables sidebar entry for `total` in `summarize`, rename it to `score`, and apply.
   Expected: source shows `score := square(limit)` and every `total` use in the function becomes `score`.

8. Press undo repeatedly through at least 20 mixed edits.
   Expected: each step restores the exact previous Jet source and the status says what changed, such as `Undo: insert print`.

9. Press redo after several undos.
   Expected: Canvas reapplies the same checked source states in order.

10. Click `Check`.
    Expected: the diagnostics panel stays visible; clean source shows no errors, and a bad inline name shows the full Jet diagnostic with What/Why/Fix text.

11. Click `Run`, then confirm the command if prompted.
    Expected: the run HUD finishes as passed and the receipt shows stdout from `fn run()`.

12. Select a node, switch to source view, then switch back to graph view.
    Expected: the same node remains selected and source still matches the graph.

13. Keep editing for several more insert, wire, inline-edit, rename, and undo actions.
    Expected: after every action the graph equals a fresh projection of the current source; any rejected edit leaves source unchanged and shows a persistent diagnostic.
