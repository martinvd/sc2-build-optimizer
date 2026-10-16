//! Turning a goal like "5 roaches" into the set of things that must be built.

use crate::data::{self, Id, N, Source};

/// What the player starts with on a standard ladder map.
pub const START_DRONES: u16 = 12;

pub struct Plan {
    /// Completed count wanted at the end, per item (individual units).
    pub goal: [u16; N],
    /// Actions that must be started to reach the goal, beyond the starting
    /// units. Economy items (drones, overlords, ...) only appear here when the
    /// goal names them; the search adds more of them freely.
    pub required: [u16; N],
    /// Total gas the build spends.
    pub gas: u32,
}

pub fn parse_goal(input: &str) -> Result<Vec<(Id, u16)>, String> {
    let mut goal = Vec::new();
    for part in input.split([',', '+', ';']).flat_map(|p| p.split(" and ")) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (count, name) = split_count(part);
        let id = data::lookup(name).ok_or_else(|| format!("unknown unit or structure: {name:?}"))?;
        if count == 0 {
            return Err(format!("count for {name:?} must be at least 1"));
        }
        goal.push((id, count));
    }
    if goal.is_empty() {
        return Err("empty goal".into());
    }
    Ok(goal)
}

/// Accepts "5 roach", "5x roach", "5roach", "roach=5", "roach 5", "roach".
fn split_count(part: &str) -> (u16, &str) {
    if let Some((name, n)) = part.split_once('=') {
        return (n.trim().parse().unwrap_or(0), name.trim());
    }
    let digits = part.find(|c: char| !c.is_ascii_digit()).unwrap_or(part.len());
    if digits > 0 {
        let rest = part[digits..].trim_start();
        let rest = rest.strip_prefix(['x', '*']).filter(|r| !r.is_empty()).unwrap_or(rest);
        return (part[..digits].parse().unwrap_or(0), rest.trim());
    }
    if let Some((name, n)) = part.rsplit_once(' ') {
        if let Ok(n) = n.parse() {
            return (n, name.trim());
        }
    }
    (1, part)
}

impl Plan {
    pub fn new(goal: &[(Id, u16)]) -> Plan {
        let mut want = [0u16; N];
        for &(id, n) in goal {
            want[id as usize] += n;
        }
        let goal_counts = want;

        // A morph consumes its source unit, so the source has to be made too.
        let mut units = want;
        for id in data::all() {
            if let Source::Morph(src) = data::def(id).source {
                units[src as usize] += units[id as usize];
            }
        }

        let mut required = [0u16; N];
        for id in data::all() {
            let have = initial_count(id);
            let need = units[id as usize].saturating_sub(have);
            required[id as usize] = need.div_ceil(data::def(id).yields);
        }

        // Pull in the tech each required item depends on.
        let mut stack: Vec<Id> = data::all().filter(|&id| required[id as usize] > 0).collect();
        while let Some(id) = stack.pop() {
            let def = data::def(id);
            let building = match def.source {
                Source::Building(b) => Some(b),
                _ => None,
            };
            for &r in def.requires.iter().chain(building.iter()) {
                if required[r as usize] == 0 && initial_count(r) == 0 {
                    required[r as usize] = 1;
                    stack.push(r);
                }
            }
        }

        let gas = data::all()
            .filter(|&id| !data::is_economy(id))
            .map(|id| required[id as usize] as u32 * data::def(id).gas)
            .sum();

        Plan { goal: goal_counts, required, gas }
    }

    pub fn describe(goal: &[(Id, u16)]) -> String {
        goal.iter()
            .map(|&(id, n)| format!("{n} {}", data::def(id).name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn initial_count(id: Id) -> u16 {
    match id {
        Id::Drone => START_DRONES,
        Id::Overlord | Id::Hatchery => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Id::*;

    #[test]
    fn parses_common_forms() {
        assert_eq!(parse_goal("5 roaches").unwrap(), vec![(Roach, 5)]);
        assert_eq!(parse_goal("roach=5, 2x ravager").unwrap(), vec![(Roach, 5), (Ravager, 2)]);
        assert_eq!(parse_goal("ling speed and 20 lings").unwrap(), vec![(MetabolicBoost, 1), (Zergling, 20)]);
        assert!(parse_goal("5 marines").is_err());
    }

    #[test]
    fn roaches_need_pool_warren_and_gas() {
        let plan = Plan::new(&[(Roach, 5)]);
        assert_eq!(plan.required[Roach as usize], 5);
        assert_eq!(plan.required[RoachWarren as usize], 1);
        assert_eq!(plan.required[SpawningPool as usize], 1);
        assert_eq!(plan.required[Lair as usize], 0);
        assert_eq!(plan.gas, 125);
    }

    #[test]
    fn morphs_require_their_source() {
        let plan = Plan::new(&[(Roach, 2), (Ravager, 1)]);
        assert_eq!(plan.required[Roach as usize], 3);
        let plan = Plan::new(&[(Baneling, 4)]);
        assert_eq!(plan.required[Zergling as usize], 2);
        assert_eq!(plan.required[BanelingNest as usize], 1);
    }

    #[test]
    fn upgrades_require_their_building() {
        let plan = Plan::new(&[(GlialReconstitution, 1)]);
        assert_eq!(plan.required[RoachWarren as usize], 1);
        assert_eq!(plan.required[Lair as usize], 1);
    }
}
