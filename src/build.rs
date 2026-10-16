//! A finished build order in the form the CLI and the app both display. Each
//! action has the window it is in production and a category, and the build has
//! a time axis to draw them on. The goal edits the app offers live here too.
//! It is plain data, so it can leave the search thread and be printed or tested
//! without a window.

use crate::data::{self, Id};
use crate::search::Solution;
use crate::sim::{clock, ticks};

/// Seconds between time axis marks.
const MARK_INTERVAL: f64 = 30.0;

/// What an action does for the build, which decides its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Economy,
    Army,
    Structure,
    Research,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Economy, Kind::Army, Kind::Structure, Kind::Research];

    pub fn of(id: Id) -> Kind {
        if data::is_economy(id) {
            Kind::Economy
        } else if (id as usize) < Id::Hatchery as usize {
            Kind::Army
        } else if (id as usize) < Id::MetabolicBoost as usize {
            Kind::Structure
        } else {
            Kind::Research
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Economy => "Economy",
            Kind::Army => "Army",
            Kind::Structure => "Structure",
            Kind::Research => "Research",
        }
    }
}

/// One action of a build, from when it starts to when it completes.
pub struct Step {
    pub start: u32,
    pub end: u32,
    /// Supply counter as shown in game when the step starts, in whole units.
    pub supply: u32,
    pub id: Id,
}

pub struct Build {
    pub steps: Vec<Step>,
    /// Tick the whole goal is complete.
    pub finish: u32,
    pub explored: usize,
    pub proven: bool,
}

impl Build {
    pub fn new(sol: &Solution) -> Build {
        let steps = sol
            .state
            .steps()
            .iter()
            .map(|s| Step {
                start: s.tick,
                // Matches the job the simulation queues in `State::begin`.
                end: s.tick + ticks(data::def(s.id).time),
                supply: s.supply.div_ceil(2),
                id: s.id,
            })
            .collect();
        Build { steps, finish: sol.finish, explored: sol.explored, proven: sol.proven }
    }

    /// Ticks the timeline covers: until the goal is done or the last action
    /// completes, whichever is later.
    pub fn span(&self) -> u32 {
        self.steps.iter().map(|s| s.end).chain([self.finish]).max().unwrap_or(0).max(1)
    }

    /// Where `tick` falls along the timeline, from 0 to 1.
    pub fn fraction(&self, tick: u32) -> f32 {
        tick as f32 / self.span() as f32
    }

    /// Time axis marks, every 30 game seconds within the span.
    pub fn marks(&self) -> Vec<u32> {
        let span = self.span();
        (0..).map(|i| ticks(MARK_INTERVAL * i as f64)).take_while(|&t| t <= span).collect()
    }

    pub fn peak_supply(&self) -> u32 {
        self.steps.iter().map(|s| s.supply).max().unwrap_or(0)
    }

    /// The timeline as text, one bar per action, `width` columns wide.
    pub fn render_timeline(&self, width: usize) -> String {
        let col = |tick: u32| ((self.fraction(tick) * width as f32).round() as usize).min(width);
        let name_width = self.steps.iter().map(|s| data::def(s.id).name.len()).max().unwrap_or(0);
        let gutter = 6 + 1 + 4 + 1 + name_width + 1;

        let mut axis = vec![' '; width + 6];
        for t in self.marks() {
            for (i, c) in clock(t).chars().enumerate() {
                if let Some(slot) = axis.get_mut(col(t) + i) {
                    *slot = c;
                }
            }
        }
        let mut out = format!("{:gutter$}{}\n", "", axis.iter().collect::<String>().trim_end());

        let done = col(self.finish);
        for step in &self.steps {
            let (from, to) = (col(step.start), col(step.end).max(col(step.start) + 1));
            let bar: String = (0..=width)
                .map(|c| match c {
                    _ if (from..to).contains(&c) => '█',
                    _ if c == done => '│',
                    _ => '·',
                })
                .collect();
            out += &format!(
                "{:>6} {:>4} {:<name_width$} {bar}\n",
                clock(step.start),
                step.supply,
                data::def(step.id).name
            );
        }
        out
    }
}

/// The goal as input text that parses back to the same goal.
pub fn goal_text(goal: &[(Id, u16)]) -> String {
    goal.iter().map(|&(id, n)| format!("{n} {}", data::def(id).aliases[0])).collect::<Vec<_>>().join(", ")
}

/// Changes the goal by `actions` of `id`, where one zergling action makes two
/// zerglings. Entries that drop to zero leave the goal.
pub fn adjust(goal: &mut Vec<(Id, u16)>, id: Id, actions: i32) {
    let delta = data::def(id).yields as i32 * actions;
    match goal.iter().position(|(g, _)| *g == id) {
        Some(i) => {
            let n = goal[i].1 as i32 + delta;
            if n <= 0 {
                goal.remove(i);
            } else {
                goal[i].1 = n as u16;
            }
        }
        None if delta > 0 => goal.push((id, delta as u16)),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Plan, parse_goal};
    use crate::progress::Progress;
    use crate::search::search;
    use crate::sim::Config;

    #[test]
    fn goal_text_parses_back() {
        let goal: Vec<(Id, u16)> = data::all().zip(1..).collect();
        assert_eq!(parse_goal(&goal_text(&goal)), Ok(goal));
    }

    #[test]
    fn adjust_counts_actions_and_drops_empty_entries() {
        let mut goal = vec![(Id::Roach, 1)];
        adjust(&mut goal, Id::Zergling, 1);
        assert_eq!(goal, [(Id::Roach, 1), (Id::Zergling, 2)]);
        adjust(&mut goal, Id::Roach, -1);
        assert_eq!(goal, [(Id::Zergling, 2)]);
        adjust(&mut goal, Id::Roach, -1);
        assert_eq!(goal, [(Id::Zergling, 2)]);
    }

    #[test]
    fn kinds_follow_the_tech_tree_sections() {
        assert_eq!(Kind::of(Id::Drone), Kind::Economy);
        assert_eq!(Kind::of(Id::Hatchery), Kind::Economy);
        assert_eq!(Kind::of(Id::Overseer), Kind::Army);
        assert_eq!(Kind::of(Id::SpawningPool), Kind::Structure);
        assert_eq!(Kind::of(Id::UltraliskCavern), Kind::Structure);
        assert_eq!(Kind::of(Id::MetabolicBoost), Kind::Research);
    }

    #[test]
    fn timeline_spans_the_build() {
        let cfg = Config {
            expand_travel: 12.0,
            max_expansions: 1,
            max_macro_hatcheries: 0,
            max_extractors: 2,
            max_queens: 2,
            horizon: ticks(900.0),
        };
        let plan = Plan::new(&parse_goal("3 roaches").unwrap());
        let sol = search(&plan, &cfg, 500, 2_000_000, &mut Progress::hidden()).unwrap();
        let build = Build::new(&sol);

        let last_roach = build.steps.iter().filter(|s| s.id == Id::Roach).map(|s| s.end).max().unwrap();
        assert_eq!(last_roach, build.finish, "the goal completes when the last roach does");
        assert!(build.steps.iter().all(|s| s.start < s.end && s.end <= build.span()));
        assert!(build.steps.windows(2).all(|w| w[0].start <= w[1].start));
        assert_eq!(build.marks()[0], 0);
        assert!(build.marks().windows(2).all(|w| w[1] - w[0] == ticks(30.0)));
        assert_eq!(build.render_timeline(60).lines().count(), build.steps.len() + 1);
    }
}
