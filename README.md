# sc2-build-optimizer

Finds the fastest StarCraft II build order to reach a goal, for Zerg, Terran and Protoss.

```
$ cargo run --release -- 10 roaches
Goal: 10 Roach (Zerg)

┌───────┬────────┬───────────────┐
│ Start │ Supply │ Action        │
├───────┼────────┼───────────────┤
│  0:13 │     12 │ Spawning Pool │
│  0:18 │     11 │ Drone         │
│  0:23 │     12 │ Drone         │
│  0:28 │     13 │ Drone         │
│  0:36 │     14 │ Overlord      │
│  0:39 │     14 │ Extractor     │
│  0:43 │     13 │ Drone         │
│  0:54 │     14 │ Drone         │
│  0:59 │     15 │ Queen         │
│  1:03 │     17 │ Drone         │
│  1:13 │     18 │ Drone         │
│  1:24 │     19 │ Overlord      │
│  1:29 │     19 │ Roach Warren  │
│  2:08 │     18 │ Roach         │
│  2:08 │     20 │ Roach         │
│  2:08 │     22 │ Roach         │
│  2:08 │     24 │ Roach         │
│  2:08 │     26 │ Roach         │
│  2:08 │     28 │ Roach         │
│  2:19 │     30 │ Overlord      │
│  2:37 │     30 │ Roach         │
│  2:37 │     32 │ Roach         │
│  2:37 │     34 │ Roach         │
│  2:37 │     36 │ Roach         │
└───────┴────────┴───────────────┘

10 Roach done at 2:56 game time.
Fastest build under the model (827377 states explored).
```

While it searches, a status line on stderr shows the pass, states explored and the
best build found so far. It only appears when stderr is a terminal.

Run it with no arguments to open the desktop app, built with
[GPUI Kit](https://gpui-kit.com). Pick a race, type a goal or click units to add them,
adjust the model settings, and press Enter. The app draws the fastest build as a timeline of
when each action is in production. `--timeline` prints the same chart in the terminal.

Goals accept counts and common names: `"5 roaches"`, `"roach=8, ravager=2"`,
`"20 lings and ling speed"`, `"6 marines and stim"`, `"2 tanks"`, `"4 stalkers and blink"`.
The goal's units decide the race. Names any race could mean, like `"gas"` or
`"worker"`, follow the rest of the goal, or `--race` (Zerg by default). Run with
`--help` for options.

## How it works

- **`data.rs`**: all three tech trees with costs, build times (real seconds at Faster
  speed; Terran and Protoss checked against patch 5.0.17) and game constants (mining
  rates, larva, injects, MULEs, chrono boost, warp-in). Verify numbers here against
  Liquipedia, since the rest of the code depends on them.
- **`plan.rs`**: parses a goal and turns it into everything that must be built,
  including tech prerequisites, morph sources (ravagers need roaches) and add-ons
  (marauders need a barracks tech lab).
- **`sim.rs`**: a 0.25s tick simulation. Workers mine, and the simulation puts workers
  on gas only while the build still needs gas. Queens inject, orbital commands call
  down MULEs, and nexuses chrono boost the structure with the most production left,
  all whenever they have the energy. Each production structure tracks its own
  queue, so reactors train two units at once and warp gates cool down.
- **`build.rs`**: the finished build that the CLI and the app both display. It
  holds each action's production window and category, the time axis, and goal editing.
- **`app.rs`**: the desktop app.
- **`search.rs`**: chooses what to start next. Every action starts at the earliest
  moment it can. The search runs in four passes:
  1. A greedy build drones to a fixed count, then techs straight up. It tries
     every drone count and keeps the fastest.
  2. A beam search keeps the most promising states at each depth.
  3. A best-first search with loose state merging gives a near-optimal bound.
  4. A best-first search with strict merging improves on that bound or confirms it.
     Admissible lower bounds (tech critical path, gas, larva, minerals) prune it.

The search decides on its own about extra workers, supply, gas, and taking an
expansion. Zerg adds queens; Terran adds orbital commands, extra barracks,
factories and starports, and reactors or tech labs; Protoss adds extra gateways,
robotics facilities and stargates, and Warp Gate.

## Model assumptions

- 12 workers, 1 town hall, 50 minerals at 0:00 (plus 1 overlord for Zerg).
  Workers mine at 0.94/s for 2 per patch, 0.40/s for a third, and gas at 0.89/s
  per worker.
- Expanding costs the town hall plus the mining lost while a worker walks to
  the natural (`--expand-travel`, default 12s; this depends on the map).
- Workers building in the main lose no walking time. A drone is consumed, an SCV
  mines nothing until its structure is done, and a probe loses no time at all.
- Larva, inject and energy follow the game rules. A new hatchery spawns with 1
  larva; every nexus, new ones included, starts with 50 energy.
- Chrono boost always goes to the structure with the most production left, which
  is a reasonable habit rather than the best possible use. Gateways turn into
  warp gates the moment Warp Gate finishes, and units warp in at a fast field.
- Optional items that cost gas (a factory, a reactor, Warp Gate) have workers
  start mining their gas only once the search picks them.
- "Fastest under the model" means no build the model allows is faster, apart
  from what the state merging drops. That merging is a heuristic: it ignores
  larva timers, energy and warp gate cooldowns.

## Limitations

- Large goals (20+ units) hit the default search budget, and so do many Terran
  and Protoss goals with several production structures to choose from. You still
  get the best build found, labeled as not proven. Raise `--max-states` to search
  longer, at roughly 1 GB of memory per million states.
- Not modeled: archons, motherships, Terran macro command centers, scans and
  supply drops, lifting off and swapping add-ons, and proxy structures.
