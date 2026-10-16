mod data;
mod plan;
mod search;
mod sim;

use plan::Plan;
use sim::{Config, TICKS_PER_SEC, ticks};
use std::process::ExitCode;

const USAGE: &str = "\
Find the fastest Zerg build order to reach a goal.

Usage: sc2buildmaker [options] <goal>

Goal examples:
  \"5 roaches\"
  \"roach=8, ravager=2\"
  \"20 lings and ling speed\"

Options:
  --expand-travel <sec>   Drone walk time to the natural (default 12)
  --max-expansions <n>    Expansions the search may take (default 1)
  --macro-hatcheries <n>  In-base macro hatcheries allowed (default 0)
  --max-extractors <n>    Extractors allowed (default 2)
  --max-queens <n>        Queens allowed for injects (default 2)
  --beam-width <n>        States kept per step by the quick first pass (default 2000)
  --max-states <n>        Exact search budget before settling for the best found (default 1000000)
  -h, --help              Show this help";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut cfg = Config {
        expand_travel: 12.0,
        max_expansions: 1,
        max_macro_hatcheries: 0,
        max_extractors: 2,
        max_queens: 2,
        horizon: ticks(20.0 * 60.0),
    };
    let mut max_states = 1_000_000;
    let mut beam_width = 2000;
    let mut goal_words = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> Result<String, String> {
            args.next().ok_or_else(|| format!("{name} needs a value"))
        };
        let number = |name: &str, v: String| v.parse::<f64>().map_err(|_| format!("{name}: not a number: {v}"));
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--expand-travel" => cfg.expand_travel = number(&arg, value(&arg)?)?,
            "--max-expansions" => cfg.max_expansions = number(&arg, value(&arg)?)? as u16,
            "--macro-hatcheries" => cfg.max_macro_hatcheries = number(&arg, value(&arg)?)? as u16,
            "--max-extractors" => cfg.max_extractors = number(&arg, value(&arg)?)? as u16,
            "--max-queens" => cfg.max_queens = number(&arg, value(&arg)?)? as u16,
            "--beam-width" => beam_width = number(&arg, value(&arg)?)? as usize,
            "--max-states" => max_states = number(&arg, value(&arg)?)? as usize,
            a if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ => goal_words.push(arg),
        }
    }

    let goal = plan::parse_goal(&goal_words.join(" "))?;
    let plan = Plan::new(&goal);
    println!("Goal: {}\n", Plan::describe(&goal));

    let sol = search::search(&plan, &cfg, beam_width, max_states)
        .ok_or("no build order reaches this goal within 20 minutes")?;

    println!("  Time  Supply  Action");
    for step in sol.state.steps() {
        println!("  {}  {:>6}  {}", clock(step.tick), step.supply.div_ceil(2), data::def(step.id).name);
    }
    println!("\nGoal complete at {}.", clock(sol.finish));
    if sol.proven {
        println!("Fastest build under the model ({} states explored).", sol.explored);
    } else {
        println!("Search budget hit after {} states; this is the best found, not proven fastest.", sol.explored);
    }
    Ok(())
}

fn clock(tick: u32) -> String {
    let secs = tick / TICKS_PER_SEC;
    format!("{:>2}:{:02}", secs / 60, secs % 60)
}
