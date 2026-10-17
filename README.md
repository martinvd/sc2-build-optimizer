# sc2-build-optimizer

Finds the fastest StarCraft II build order to reach a goal, for Zerg, Terran and Protoss.

## Usage

```
cargo run --release -- 10 roaches
```

This prints the build as a table of start times, supply and actions, plus the time the
goal is done. Add `--timeline` for a chart of when each action is in production.

Run with no arguments to open the desktop app, built with [GPUI Kit](https://gpui-kit.com).

Goals accept counts and common names, for example `"5 roaches"`, `"roach=8, ravager=2"`,
`"20 lings and ling speed"`, `"6 marines and stim"` or `"4 stalkers and blink"`. The units
in the goal decide the race. Run with `--help` for all options.

## How it works

A 0.25s tick simulation models mining, supply, larva, injects, MULEs, chrono boost and
warp-ins. A search on top of it picks what to start next, and decides on its own about
extra workers, supply, gas, production and expanding.

| File        | Role                                              |
| ----------- | ------------------------------------------------- |
| `data.rs`   | Tech trees, costs, build times and game constants |
| `plan.rs`   | Turns a goal into everything that must be built   |
| `sim.rs`    | The simulation                                    |
| `search.rs` | The build order search                            |
| `build.rs`  | The finished build shown by the CLI and the app   |
| `app.rs`    | The desktop app                                   |

## Limitations

- Large goals (20+ units) can hit the search budget. You still get the best build found,
  marked as not proven. Raise `--max-states` to search longer (about 1 GB per million states).
- The start is 12 workers and 1 town hall on a fixed map model; expansion walking time
  is set with `--expand-travel`.
- Not modeled: archons, motherships, macro command centers, scans, supply drops,
  add-on swapping and proxy structures.
