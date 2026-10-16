# sc2buildmaker

Finds the fastest StarCraft II build order to reach a goal. Zerg for now.

```
$ cargo run --release -- 10 roaches
Goal: 10 Roach

  Time  Supply  Action
   0:13      12  Spawning Pool
   0:18      11  Drone
   ...
   1:29      19  Roach Warren
   2:08      18  Roach
   ...
Goal complete at  2:56.
```

Goals accept counts and common names: `"5 roaches"`, `"roach=8, ravager=2"`,
`"20 lings and ling speed"`, `"5 mutas"`, `"glial"`. Run with `--help` for options.

## How it works

- **`data.rs`**: the Zerg tech tree with costs, build times (real seconds at Faster
  speed, LotV 5.0.x) and game constants (mining rates, larva, injects). Verify
  numbers here against Liquipedia; everything else builds on them.
- **`plan.rs`**: turns a goal into everything that must be built, including tech
  prerequisites and morph sources (ravagers need roaches).
- **`sim.rs`**: a 0.25s tick simulation. Drones mine, gas workers are assigned only
  while the build still needs gas, and queens inject whenever they have energy.
- **`search.rs`**: chooses what to start next. Every action starts at the earliest
  moment it can. The search runs in four passes:
  1. A greedy build and a beam search give a decent build quickly.
  2. A best-first search with loose state merging gives a near-optimal bound.
  3. A best-first search with strict merging improves on that bound or confirms it.
     Admissible lower bounds (tech critical path, gas, larva, minerals) prune it.

The search decides on its own about extra drones, overlords, queens, extractors
and taking an expansion.

## Model assumptions

- 12 drones, 1 hatchery, 1 overlord, 50 minerals at 0:00. Workers mine at
  0.94/s for 2 per patch, 0.40/s for a third, and gas at 0.89/s per worker.
- Expanding costs the 300 minerals plus the mining lost while a drone walks to
  the natural (`--expand-travel`, default 12s; this depends on the map).
- Drones building in the main lose no walking time. Larva, inject and queen
  energy follow the game rules. A new hatchery spawns with 1 larva.
- "Fastest under the model" means no build the model allows is faster, apart
  from what the state merging drops. That merging is a heuristic: it ignores
  larva timers and queen energy.

## Limitations

- Large goals (20+ units) hit the default search budget. You still get the best
  build found, labeled as not proven. Raise `--max-states` to search longer, at
  roughly 1.5 GB of memory per million states.
- Only Zerg. Adding a race means a new data table and its production rules
  (chrono boost, MULEs, add-ons).
