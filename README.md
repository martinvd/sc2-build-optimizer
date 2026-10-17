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

A simulation steps through the game in 0.25s ticks. It models mining, supply, larva,
injects, MULEs, chrono boost and warp-ins. A search uses it to pick what to start next,
including workers, supply, gas and expansions.

Each build starts with 12 workers and 1 town hall. By default the search stays within
these limits, which you can change in the app's Model settings or with the CLI options:

- 1 expansion, with a 12 second worker walk to it
- 2 gas buildings
- 3 production structures of each type
- 2 queens and no macro hatcheries
- 1 million search states, about 1 GB of memory

Goals of 20 or more units can hit the state limit. You then get the best build found, and
the output marks it as not proven fastest.

The simulation does not model archons, motherships, macro command centers, scans, supply
drops, add-on swapping or proxy structures.
