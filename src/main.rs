mod app;
mod build;
mod data;
mod plan;
mod progress;
mod search;
mod sim;
mod table;

use build::Build;
use plan::Plan;
use progress::Progress;
use sim::{Config, clock, ticks};
use std::process::ExitCode;
use table::{Align, Table};

const USAGE: &str = "\
Find the fastest StarCraft II build order to reach a goal.

Usage: sc2-build-optimizer [options] <goal>
       sc2-build-optimizer      (no arguments opens the app)

Goal examples:
  \"5 roaches\"
  \"roach=8, ravager=2\"
  \"20 lings and ling speed\"
  \"6 marines and stim\"
  \"4 stalkers, blink\"

Options:
  --race <race>           Zerg, Terran or Protoss, for names any race could mean,
                          like \"gas\" (default: the goal's race, else Zerg)
  --expand-travel <sec>   Worker walk time to the natural (default 12)
  --max-expansions <n>    Expansions the search may take (default 1)
  --max-extractors <n>    Gas buildings allowed (default 2)
  --max-production <n>    Terran and Protoss structures allowed of each
                          production type, like barracks (default 3)
  --macro-hatcheries <n>  Zerg in-base macro hatcheries allowed (default 0)
  --max-queens <n>        Zerg queens allowed for injects (default 2)
  --beam-width <n>        States kept per step by the quick first pass (default 2000)
  --max-states <n>        Exact search budget before settling for the best found (default 1000000)
  --timeline              Also chart when each action is in production
  -h, --help              Show this help";

fn main() -> ExitCode {
    if std::env::args().len() == 1 {
        app::run();
        return ExitCode::SUCCESS;
    }
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
        max_production: 3,
        horizon: ticks(20.0 * 60.0),
    };
    let mut race = data::Race::Zerg;
    let mut max_states = 1_000_000;
    let mut beam_width = 2000;
    let mut timeline = false;
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
            "--max-production" => cfg.max_production = number(&arg, value(&arg)?)? as u16,
            "--race" => {
                let v = value(&arg)?;
                race = data::Race::parse(&v).ok_or_else(|| format!("--race: unknown race {v}"))?;
            }
            "--beam-width" => beam_width = number(&arg, value(&arg)?)? as usize,
            "--max-states" => max_states = number(&arg, value(&arg)?)? as usize,
            "--timeline" => timeline = true,
            a if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ => goal_words.push(arg),
        }
    }

    let goal = plan::parse_goal(&goal_words.join(" "), race)?;
    let plan = Plan::new(&goal);
    plan.check()?;
    println!("Goal: {} ({})\n", Plan::describe(&goal), plan.race.name());

    let sol = search::search(&plan, &cfg, beam_width, max_states, &mut Progress::stderr())
        .ok_or("no build order reaches this goal within 20 minutes")?;

    let build = Build::new(&sol);
    let mut table = Table::new(&[
        ("Start", Align::Right),
        ("Done", Align::Right),
        ("Supply", Align::Right),
        ("Action", Align::Left),
        ("Kind", Align::Left),
    ]);
    for step in &build.steps {
        table.row(vec![
            clock(step.start),
            clock(step.end),
            step.supply.to_string(),
            data::def(step.id).name.to_string(),
            build::Kind::of(step.id).label().to_string(),
        ]);
    }
    print!("{}", table.render());
    println!("\n{}", build.summary());
    if timeline {
        println!("\n{}", build.render_timeline(72));
    }
    println!("\n{} done at {} game time.", Plan::describe(&goal), clock(build.finish));
    if build.proven {
        println!("Fastest build under the model ({} states explored).", build.explored);
    } else {
        println!("Search budget hit after {} states; this is the best found, not proven fastest.", build.explored);
    }
    Ok(())
}
