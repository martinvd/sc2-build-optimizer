# sc2-build-optimizer

Finds the fastest StarCraft II build order to reach a goal, for Zerg, Terran and Protoss.
The same binary runs as a desktop app or as a command-line tool.

## Usage

### Desktop app

```
cargo run --release
```

Run with no arguments to open the desktop app, built with [GPUI Kit](https://gpui-kit.com).

### Command line

```
cargo run --release -- 10 roaches
```

Pass a goal to run in the terminal instead. This prints the build as a table of start
times, supply and actions, plus the time the goal is done. Add `--timeline` for a chart of
when each action is in production. Run with `--help` for all options.

### Goals

Goals accept counts and common names, for example `"5 roaches"`, `"roach=8, ravager=2"`,
`"20 lings and ling speed"`, `"6 marines and stim"` or `"4 stalkers and blink"`. The units
in the goal set the race.

## How it works

A 0.25s tick simulation models mining, supply, larva, injects, MULEs, chrono boost and
warp-ins. A search runs the simulation to pick which action to start next. It also chooses
when to add workers, supply, gas, production and expansions. By default it takes at most 1
expansion, 2 gas buildings, 3 production structures of each type and 2 queens, and builds
no macro hatcheries. Change these limits in the app's Model settings, or see `--help` for
the CLI options.

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
  and the output marks it as not proven. Raise `--max-states` to search longer (about 1 GB
  per million states).
- Every build starts with 12 workers and 1 town hall, with fixed mining rates per mineral
  patch. Set the worker walk time to the natural with `--expand-travel`.
- Not modeled: archons, motherships, macro command centers, scans, supply drops,
  add-on swapping and proxy structures.
