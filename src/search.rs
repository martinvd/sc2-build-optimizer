//! Best-first branch and bound over build orders.
//!
//! A node is the game state right after starting an action. Children start one
//! more action at the earliest moment it becomes possible, so every build order
//! is explored with its actions in start-time order. Nodes are expanded in
//! order of a lower bound on the goal's completion time, which means the first
//! finished build popped from the queue is the fastest one the model allows.
//!
//! Two things keep the tree small:
//! - Economy actions are only offered when they can still pay off (no drones
//!   past saturation, no overlords while supply is plentiful, ...).
//! - Among states that started the same multiset of actions, the search drops
//!   one that is later, poorer, no better off in larva and has nothing finishing
//!   sooner than another.

use crate::data::{self, Id, N, Source};
use crate::plan::Plan;
use crate::progress::Progress;
use crate::sim::{Config, State, TICKS_PER_SEC, ticks};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub struct Solution {
    pub state: State,
    pub finish: u32,
    pub explored: usize,
    /// False when the search ran out of budget and this is the best found.
    pub proven: bool,
}

pub fn search(
    plan: &Plan,
    cfg: &Config,
    beam_width: usize,
    max_states: usize,
    progress: &mut Progress,
) -> Option<Solution> {
    let root = State::start(plan, cfg);
    let greedy = greedy_bound(&root, plan, cfg);
    let beamed = beam(&root, plan, cfg, beam_width, greedy.as_ref().map(|(t, _)| *t), progress);
    let quick = [greedy, beamed].into_iter().flatten().min_by_key(|(t, _)| *t);
    let (rough, _) = best_first(&root, plan, cfg, quick, Pruning::Loose, max_states, progress);
    let (best, outcome) = best_first(&root, plan, cfg, rough, Pruning::Strict, max_states, progress);
    progress.clear();
    best.map(|(finish, state)| Solution {
        state,
        finish,
        explored: outcome.explored,
        proven: outcome.exhausted,
    })
}

/// How aggressively states that started the same actions are merged.
#[derive(Clone, Copy, PartialEq)]
enum Pruning {
    /// Ignores when in-progress work finishes. Much faster, but can drop the
    /// fastest build, so it only serves as a bound for the strict pass.
    Loose,
    Strict,
}

struct Outcome {
    explored: usize,
    /// Whether every state that could beat the result was explored.
    exhausted: bool,
}

type Seen = HashMap<([u16; N], [u16; N]), Vec<Mark>>;

fn best_first(
    root: &State,
    plan: &Plan,
    cfg: &Config,
    mut best: Option<(u32, State)>,
    pruning: Pruning,
    max_states: usize,
    progress: &mut Progress,
) -> (Option<(u32, State)>, Outcome) {
    let pass = if pruning == Pruning::Strict { "Exact search" } else { "Rough search" };
    let mut arena: Vec<Option<State>> = vec![Some(root.clone())];
    let mut queue = BinaryHeap::new();
    queue.push((Reverse(0u32), 0u16, 0usize));
    let mut seen = Seen::new();
    let mut explored = 0;

    while let Some((Reverse(f), _, idx)) = queue.pop() {
        if best.as_ref().is_some_and(|(t, _)| *t <= f) {
            break;
        }
        let state = arena[idx].take().unwrap();
        if is_terminal(&state, plan) {
            best = Some((f, state));
            break;
        }
        explored += 1;
        progress.update(pass, explored, best.as_ref().map(|(t, _)| *t));
        if explored > max_states {
            return (best, Outcome { explored, exhausted: false });
        }

        for child in state.branch(&candidates(&state, plan, cfg), cfg.horizon) {
            let f = if is_terminal(&child, plan) {
                let Some(t) = child.finish(plan, cfg.horizon) else { continue };
                t
            } else {
                lower_bound(&child, plan, cfg)
            };
            if best.as_ref().is_some_and(|(t, _)| *t <= f) {
                continue;
            }
            if !record(&mut seen, &child, pruning) {
                continue;
            }
            let depth: u16 = child.started.iter().sum();
            arena.push(Some(child));
            queue.push((Reverse(f), depth, arena.len() - 1));
        }
    }

    (best, Outcome { explored, exhausted: true })
}

fn is_terminal(s: &State, plan: &Plan) -> bool {
    data::all().all(|id| {
        let i = id as usize;
        if data::is_economy(id) {
            s.count_with_pending(id) >= plan.goal[i]
        } else {
            s.started[i] >= plan.required[i]
        }
    })
}

fn remaining(s: &State, plan: &Plan, id: Id) -> u16 {
    if data::is_economy(id) {
        0
    } else {
        plan.required[id as usize].saturating_sub(s.started[id as usize])
    }
}

fn hatcheries_total(s: &State) -> u16 {
    s.hatches.len() as u16 + s.pending[Id::Hatchery as usize] + s.pending[Id::MacroHatchery as usize]
}

fn drone_cap(s: &State, plan: &Plan) -> u16 {
    let bases = s.count_with_pending(Id::Hatchery);
    let extractors = s.started[Id::Extractor as usize];
    let for_buildings: u16 = data::all()
        .filter(|&id| data::def(id).source == Source::Worker)
        .map(|id| remaining(s, plan, id))
        .sum();
    let saturation = bases * 2 * data::PATCHES_PER_BASE as u16 + extractors * data::WORKERS_PER_GEYSER as u16;
    (saturation + for_buildings).max(plan.goal[Id::Drone as usize])
}

fn queen_cap(s: &State, plan: &Plan, cfg: &Config) -> u16 {
    cfg.max_queens.min(hatcheries_total(s)).max(plan.goal[Id::Queen as usize])
}

/// Actions worth trying next from `s`.
fn candidates(s: &State, plan: &Plan, cfg: &Config) -> Vec<Id> {
    let started = |id: Id| s.started[id as usize];
    let goal = |id: Id| plan.goal[id as usize];
    let bases = s.count_with_pending(Id::Hatchery);
    let gas_left = s.gas_still_needed() > 0.0;

    data::all()
        .filter(|&id| match id {
            Id::Drone => s.count_with_pending(Id::Drone) < drone_cap(s, plan),
            Id::Overlord => overlord_useful(s, plan, cfg),
            Id::Queen => started(id) < queen_cap(s, plan, cfg),
            Id::Extractor => {
                let cap = cfg.max_extractors.min(bases * data::GEYSERS_PER_BASE);
                gas_left && started(id) < cap.max(goal(id))
            }
            Id::Hatchery => started(id) < cfg.max_expansions.max(goal(id).saturating_sub(1)),
            Id::MacroHatchery => started(id) < cfg.max_macro_hatcheries.max(goal(id)),
            _ => remaining(s, plan, id) > 0,
        })
        .filter(|&id| s.status(id) != crate::sim::Status::Never)
        .collect()
}

/// Overlords only when supply is getting tight and something still needs it.
fn overlord_useful(s: &State, plan: &Plan, cfg: &Config) -> bool {
    let total = s.supply_cap + s.pending_supply();
    if total >= data::MAX_SUPPLY {
        return false;
    }
    if s.count_with_pending(Id::Overlord) < plan.goal[Id::Overlord as usize] {
        return true;
    }
    let free = total - s.supply_used.min(total);
    let plan_supply: u32 = data::all()
        .map(|id| remaining(s, plan, id) as u32 * data::def(id).supply)
        .sum();
    let drones = drone_cap(s, plan).saturating_sub(s.count_with_pending(Id::Drone)) as u32;
    let queens = queen_cap(s, plan, cfg).saturating_sub(s.started[Id::Queen as usize]) as u32;
    let demand = plan_supply + drones * data::def(Id::Drone).supply + queens * data::def(Id::Queen).supply;
    free < 16 && free < demand
}

/// Optimistic completion tick for the goal from `s`.
fn lower_bound(s: &State, plan: &Plan, cfg: &Config) -> u32 {
    let mut memo = [None; N];
    let mut bound = s.tick;

    for id in data::all() {
        let want = plan.goal[id as usize];
        if want == 0 || s.count(id) >= want {
            continue;
        }
        let t = if s.count_with_pending(id) >= want {
            s.last_finish(id).unwrap_or(s.tick)
        } else {
            next_ready(s, id, &mut memo)
        };
        bound = bound.max(t);
    }

    // Gas: even with every allowed geyser saturated right away.
    let gas_needed = s.gas_still_needed();
    if gas_needed > 0.0 {
        let first_geyser = if s.count(Id::Extractor) > 0 {
            s.tick
        } else {
            s.first_finish(Id::Extractor).unwrap_or(s.tick + ticks(data::def(Id::Extractor).time))
        };
        let rate = data::GAS_RATE * (data::WORKERS_PER_GEYSER * cfg.max_extractors.max(1) as u32) as f64;
        let shortest = shortest_remaining(s, plan, |id| data::def(id).gas > 0);
        bound = bound.max(first_geyser + ticks(gas_needed / rate) + shortest);
    }

    // Larva: every hatchery we could ever have, spawning and injected nonstop.
    let larva_units: u32 = data::all()
        .filter(|&id| data::def(id).source == Source::Larva)
        .map(|id| remaining(s, plan, id) as u32)
        .sum();
    if larva_units > s.larva() {
        let hatches = hatcheries_total(s)
            + cfg.max_expansions.saturating_sub(s.started[Id::Hatchery as usize])
            + cfg.max_macro_hatcheries.saturating_sub(s.started[Id::MacroHatchery as usize]);
        let per_sec = 1.0 / data::LARVA_INTERVAL + data::INJECT_LARVA as f64 / data::INJECT_TIME;
        let wait = (larva_units - s.larva()) as f64 / (hatches as f64 * per_sec);
        let shortest = shortest_remaining(s, plan, |id| data::def(id).source == Source::Larva);
        bound = bound.max(s.tick + ticks(wait) + shortest);
    }

    if let Some(t) = mineral_bound(s, plan, cfg) {
        bound = bound.max(t);
    }

    bound
}

/// Earliest tick the remaining plan could be paid for in minerals, then built.
///
/// The bound overestimates income. Current and in-production drones mine at full
/// rate, and every larva the hatcheries could ever spawn turns into a drone the
/// moment it appears. Such a drone still only counts for what it nets by the
/// deadline (mining minus its 50 minerals), so short horizons get no credit.
fn mineral_bound(s: &State, plan: &Plan, cfg: &Config) -> Option<u32> {
    let cost: f64 = data::all()
        .map(|id| remaining(s, plan, id) as f64 * data::def(id).minerals as f64)
        .sum();
    let needed = cost - s.minerals;
    if needed <= 0.0 {
        return None;
    }

    let drone = data::def(Id::Drone);
    let income = s.mineral_income() + s.pending[Id::Drone as usize] as f64 * data::MINERAL_RATE;
    let hatches = hatcheries_total(s) + cfg.max_expansions.saturating_sub(s.started[Id::Hatchery as usize]);
    let larva_per_sec =
        hatches as f64 * (1.0 / data::LARVA_INTERVAL + data::INJECT_LARVA as f64 / data::INJECT_TIME);
    let larva_now = s.larva() as f64;

    let mined_by = |t: f64| {
        let mut total = income * t;
        for k in 0.. {
            let appears = if (k as f64) < larva_now { 0.0 } else { (k as f64 - larva_now + 1.0) / larva_per_sec };
            let net = data::MINERAL_RATE * (t - appears - drone.time) - drone.minerals as f64;
            if net <= 0.0 {
                break;
            }
            total += net;
        }
        total
    };

    let (mut lo, mut hi) = (0.0, 1.0);
    while mined_by(hi) < needed {
        hi *= 2.0;
        if hi > 3600.0 {
            return None;
        }
    }
    while hi - lo > 1.0 / TICKS_PER_SEC as f64 {
        let mid = (lo + hi) / 2.0;
        if mined_by(mid) >= needed { hi = mid } else { lo = mid }
    }
    let shortest = shortest_remaining(s, plan, |id| data::def(id).minerals > 0);
    Some(s.tick + (lo * TICKS_PER_SEC as f64) as u32 + shortest)
}

fn shortest_remaining(s: &State, plan: &Plan, filter: impl Fn(Id) -> bool) -> u32 {
    data::all()
        .filter(|&id| remaining(s, plan, id) > 0 && filter(id))
        .map(|id| ticks(data::def(id).time))
        .min()
        .unwrap_or(0)
}

/// Earliest tick one more `id` could be complete, given unlimited resources.
fn next_ready(s: &State, id: Id, memo: &mut [Option<u32>; N]) -> u32 {
    if let Some(t) = memo[id as usize] {
        return t;
    }
    let def = data::def(id);
    let mut start = s.tick;
    for &r in def.requires {
        start = start.max(available(s, r, memo));
    }
    match def.source {
        Source::Morph(src) | Source::Building(src) => start = start.max(available(s, src, memo)),
        _ => {}
    }
    let t = start + ticks(def.time);
    memo[id as usize] = Some(t);
    t
}

fn available(s: &State, id: Id, memo: &mut [Option<u32>; N]) -> u32 {
    if s.count(id) > 0 {
        s.tick
    } else if let Some(t) = s.first_finish(id) {
        t
    } else {
        next_ready(s, id, memo)
    }
}

struct Mark {
    tick: u32,
    minerals: f64,
    gas: f64,
    mineral_income: f64,
    larva: u32,
    jobs: Vec<(Id, u32)>,
}

/// Records `s` unless an equivalent state is already at least as good.
fn record(seen: &mut Seen, s: &State, pruning: Pruning) -> bool {
    let mark = Mark {
        tick: s.tick,
        minerals: s.minerals,
        gas: s.gas,
        mineral_income: s.mineral_income(),
        larva: s.larva(),
        jobs: if pruning == Pruning::Strict { s.jobs_sorted() } else { Vec::new() },
    };
    // Strictly, only states with the same work in progress are comparable.
    let pending = if pruning == Pruning::Strict { s.pending } else { [0; N] };
    let marks = seen.entry((s.started, pending)).or_default();
    if marks.iter().any(|m| dominates(m, &mark)) {
        return false;
    }
    marks.retain(|m| !dominates(&mark, m));
    marks.push(mark);
    true
}

fn dominates(a: &Mark, b: &Mark) -> bool {
    if a.tick > b.tick {
        return false;
    }
    let elapsed = (b.tick - a.tick) as f64 / TICKS_PER_SEC as f64;
    a.minerals + a.mineral_income * elapsed >= b.minerals - 1e-6
        && a.gas >= b.gas - 1e-6
        && a.mineral_income >= b.mineral_income - 1e-6
        && a.larva >= b.larva
        && a.jobs.len() == b.jobs.len()
        && a.jobs.iter().zip(&b.jobs).all(|(x, y)| x.0 == y.0 && x.1 <= y.1)
}

/// A good build found fast: the search tree explored one action at a time,
/// keeping only the `width` most promising states at each depth.
fn beam(
    root: &State,
    plan: &Plan,
    cfg: &Config,
    width: usize,
    known: Option<u32>,
    progress: &mut Progress,
) -> Option<(u32, State)> {
    let mut best: Option<(u32, State)> = None;
    let mut layer = vec![root.clone()];
    let mut explored = 0;
    while !layer.is_empty() {
        let mut seen = HashMap::new();
        let mut next = Vec::new();
        for state in &layer {
            explored += 1;
            let shown = best.as_ref().map(|(t, _)| *t).into_iter().chain(known).min();
            progress.update("Quick pass", explored, shown);
            for child in state.branch(&candidates(state, plan, cfg), cfg.horizon) {
                if is_terminal(&child, plan) {
                    if let Some(t) = child.finish(plan, cfg.horizon) {
                        if best.as_ref().is_none_or(|(b, _)| t < *b) {
                            best = Some((t, child));
                        }
                    }
                    continue;
                }
                let f = lower_bound(&child, plan, cfg);
                if best.as_ref().is_some_and(|(b, _)| *b <= f) || !record(&mut seen, &child, Pruning::Strict) {
                    continue;
                }
                next.push((f, child));
            }
        }
        next.sort_by_key(|(f, s)| (*f, s.tick));
        next.truncate(width);
        layer = next.into_iter().map(|(_, s)| s).collect();
    }
    best
}

/// A quick upper bound: drone to a fixed count, then tech straight up. Tried
/// for every drone count, keeping the fastest.
fn greedy_bound(root: &State, plan: &Plan, cfg: &Config) -> Option<(u32, State)> {
    let start = root.count(Id::Drone);
    (start..=start + 2 * data::PATCHES_PER_BASE as u16)
        .filter_map(|target| greedy(root, plan, cfg, target))
        .min_by_key(|(t, _)| *t)
}

fn greedy(root: &State, plan: &Plan, cfg: &Config, drone_target: u16) -> Option<(u32, State)> {
    let mut s = root.clone();
    loop {
        if is_terminal(&s, plan) {
            return s.finish(plan, cfg.horizon).map(|t| (t, s));
        }
        let free = (s.supply_cap + s.pending_supply()).saturating_sub(s.supply_used);
        let ids: Vec<Id> = candidates(&s, plan, cfg)
            .into_iter()
            .filter(|&id| match id {
                Id::Overlord | Id::Queen | Id::Hatchery | Id::MacroHatchery => false,
                Id::Drone => s.count_with_pending(Id::Drone) < drone_target,
                Id::Extractor => s.started[id as usize] == 0,
                _ => true,
            })
            .collect();
        let options = s.branch(&ids, cfg.horizon);
        if free <= 4 || options.is_empty() {
            s = s.wait_and_begin(Id::Overlord, cfg.horizon)?;
            continue;
        }
        s = options.into_iter().min_by_key(|c| (c.tick, c.log.as_ref().unwrap().id == Id::Drone)).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            expand_travel: 12.0,
            max_expansions: 1,
            max_macro_hatcheries: 0,
            max_extractors: 2,
            max_queens: 2,
            horizon: ticks(900.0),
        }
    }

    fn solve(goal: &[(Id, u16)]) -> Solution {
        let plan = Plan::new(goal);
        search(&plan, &cfg(), 500, 2_000_000, &mut Progress::hidden()).expect("no build found")
    }

    #[test]
    fn single_drone_is_immediate() {
        let sol = solve(&[(Id::Drone, 13)]);
        assert_eq!(sol.finish, ticks(12.0));
        assert!(sol.proven);
    }

    #[test]
    fn roach_build_respects_the_tech_tree() {
        let sol = solve(&[(Id::Roach, 5)]);
        let order: Vec<Id> = sol.state.steps().iter().map(|s| s.id).collect();
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(Id::SpawningPool) < pos(Id::RoachWarren));
        assert!(pos(Id::RoachWarren) < pos(Id::Roach));
        assert!(pos(Id::Extractor) < pos(Id::Roach));
        assert_eq!(order.iter().filter(|&&x| x == Id::Roach).count(), 5);
    }
}
