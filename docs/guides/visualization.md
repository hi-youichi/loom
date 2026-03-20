# Graph Visualization

Loom can export the structure of a **CompiledStateGraph** to text formats for debugging and inspection. This document covers DOT and text output and integration with Graphviz.

## DOT format generation

**generate_dot(graph)** returns a string in **Graphviz DOT** format:

- **digraph** with **rankdir=LR** (left-to-right).
- **START** and **END** nodes with distinct style (e.g. fillcolor=lightgreen / lightcoral).
- All graph nodes (from **graph.nodes**) as boxes.
- Edges derived from **edge_order**: START → first → … → last → END.

Conditional edges (when present) are represented in the compiled graph’s routing; the DOT output reflects the linear **edge_order** when that is the primary structure. For full conditional routing visualization, the implementation may extend to **next_map** (not shown in the minimal snippet).

Use the returned string with Graphviz (e.g. `dot -Tpng -o graph.png`) to render an image.

## Text format output

**generate_text(graph)** returns a human-readable text description of the graph (e.g. node list and edge list). Use it for quick inspection in logs or terminals without Graphviz.

## Integration with Graphviz tools

- Save the DOT string to a file and run: `dot -Tpng -o out.png`, or `dot -Tsvg -o out.svg`.
- Alternatively pipe: `generate_dot(&compiled) | dot -Tpng -o out.png` (from shell).

## Debugging and inspection

- Call **generate_dot** or **generate_text** after **compile** to verify node and edge structure.
- Helpful when adding conditional edges or new nodes to ensure START/END and routing are correct.

## Summary

| Function | Output | Use |
|----------|--------|-----|
| generate_dot | DOT string | Graphviz (dot, neato, etc.) |
| generate_text | Text summary | Logs, terminal |

Next: [Testing](testing.md) for testing strategies and examples.
