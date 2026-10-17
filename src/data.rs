//! Tech trees, costs and timings for all three races.
//!
//! All times are in real seconds at "Faster" game speed (what the in-game clock
//! shows in Legacy of the Void). Terran and Protoss values follow patch 5.0.17; check
//! them against Liquipedia when tuning, since the rest of the code depends on them.
//!
//! Supply is stored in half-supply units so a single zergling (0.5) is exact.
//!
//! Each race's items are contiguous in `Id`, ordered units, then structures,
//! then research. A game only ever holds one race, so per-item arrays are
//! indexed by `slot` within the race and sized for the largest race.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Id {
    // Zerg units
    Drone,
    Overlord,
    Zergling,
    Queen,
    Roach,
    Ravager,
    Baneling,
    Hydralisk,
    Lurker,
    Mutalisk,
    Corruptor,
    BroodLord,
    Infestor,
    SwarmHost,
    Viper,
    Ultralisk,
    Overseer,
    // Zerg structures
    Hatchery,
    MacroHatchery,
    Extractor,
    SpawningPool,
    EvolutionChamber,
    RoachWarren,
    BanelingNest,
    SpineCrawler,
    SporeCrawler,
    Lair,
    HydraliskDen,
    LurkerDen,
    InfestationPit,
    Spire,
    Hive,
    GreaterSpire,
    UltraliskCavern,
    // Zerg research
    MetabolicBoost,
    GlialReconstitution,
    CentrifugalHooks,
    GroovedSpines,
    MuscularAugments,
    PneumatizedCarapace,
    Burrow,
    MissileWeapons1,
    MeleeWeapons1,
    GroundCarapace1,
    // Terran units
    Scv,
    Marine,
    Reaper,
    Marauder,
    Ghost,
    Hellion,
    Hellbat,
    WidowMine,
    Cyclone,
    SiegeTank,
    Thor,
    Viking,
    Medivac,
    Liberator,
    Raven,
    Banshee,
    Battlecruiser,
    // Terran structures
    CommandCenter,
    OrbitalCommand,
    PlanetaryFortress,
    SupplyDepot,
    Refinery,
    Barracks,
    EngineeringBay,
    Bunker,
    MissileTurret,
    Factory,
    GhostAcademy,
    Armory,
    Starport,
    FusionCore,
    BarracksTechLab,
    BarracksReactor,
    FactoryTechLab,
    FactoryReactor,
    StarportTechLab,
    StarportReactor,
    // Terran research
    Stimpack,
    CombatShield,
    ConcussiveShells,
    InfernalPreIgniter,
    CloakingField,
    InfantryWeapons1,
    InfantryArmor1,
    VehicleWeapons1,
    ShipWeapons1,
    // Protoss units
    Probe,
    Zealot,
    Stalker,
    Sentry,
    Adept,
    HighTemplar,
    DarkTemplar,
    Observer,
    WarpPrism,
    Immortal,
    Colossus,
    Disruptor,
    Phoenix,
    Oracle,
    VoidRay,
    Tempest,
    Carrier,
    // Protoss structures
    Nexus,
    Pylon,
    Assimilator,
    Gateway,
    Forge,
    PhotonCannon,
    ShieldBattery,
    CyberneticsCore,
    TwilightCouncil,
    Stargate,
    RoboticsFacility,
    RoboticsBay,
    TemplarArchives,
    DarkShrine,
    FleetBeacon,
    // Protoss research
    WarpGate,
    Charge,
    Blink,
    ResonatingGlaives,
    GroundWeapons1,
    GroundArmor1,
    Shields1,
    ExtendedThermalLance,
    PsionicStorm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Race {
    Zerg,
    Terran,
    Protoss,
}

/// Where an item comes from, which also decides what it occupies or consumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Morphs from a larva.
    Larva,
    /// Built by a worker. A drone is consumed, an SCV stops mining until the
    /// structure is done, and a probe only warps it in.
    Worker,
    /// Morphs from a completed unit of the given type, consuming it.
    Morph(Id),
    /// Trained, researched or morphed at an idle town hall (workers, queens,
    /// lair, orbital command, ...).
    TownHall,
    /// Trained or researched at an idle structure of the given type. With a
    /// reactor attached, the structure trains two at once.
    Building(Id),
    /// Trained at an idle structure of the given type with a tech lab attached.
    TechLab(Id),
    /// An add-on built by an idle structure of the given type without one.
    AddOn(Id),
}

pub struct Def {
    pub id: Id,
    pub name: &'static str,
    /// Names the goal parser accepts. The first one is unique across races.
    pub aliases: &'static [&'static str],
    pub minerals: u32,
    pub gas: u32,
    pub time: f64,
    /// Supply cost in half units. For morphs this is the difference to the source.
    pub supply: u32,
    /// Supply provided in half units.
    pub provides: u32,
    /// Units produced per action (zerglings come in pairs).
    pub yields: u16,
    pub source: Source,
    pub requires: &'static [Id],
    /// Warp gate cooldown for gateway units once Warp Gate is researched, or 0.
    pub warp: f64,
}

const fn d(
    id: Id,
    name: &'static str,
    aliases: &'static [&'static str],
    (minerals, gas, time): (u32, u32, f64),
    supply: u32,
    source: Source,
    requires: &'static [Id],
) -> Def {
    Def { id, name, aliases, minerals, gas, time, supply, provides: 0, yields: 1, source, requires, warp: 0.0 }
}

use Id::*;
use Source::*;

pub static DEFS: [Def; 131] = [
    // Zerg
    d(Drone, "Drone", &["drone", "worker"], (50, 0, 12.0), 2, Larva, &[]),
    Def { provides: 16, ..d(Overlord, "Overlord", &["overlord", "ovi", "ovie", "supply"], (100, 0, 18.0), 0, Larva, &[]) },
    Def { yields: 2, ..d(Zergling, "Zergling", &["zergling", "ling"], (50, 0, 17.0), 2, Larva, &[SpawningPool]) },
    d(Queen, "Queen", &["queen"], (150, 0, 36.0), 4, TownHall, &[SpawningPool]),
    d(Roach, "Roach", &["roach"], (75, 25, 19.0), 4, Larva, &[RoachWarren]),
    d(Ravager, "Ravager", &["ravager"], (25, 75, 9.0), 2, Morph(Roach), &[RoachWarren]),
    d(Baneling, "Baneling", &["baneling", "bane", "bling"], (25, 25, 14.0), 0, Morph(Zergling), &[BanelingNest]),
    d(Hydralisk, "Hydralisk", &["hydralisk", "hydra"], (100, 50, 24.0), 4, Larva, &[HydraliskDen]),
    d(Lurker, "Lurker", &["lurker"], (50, 100, 18.0), 2, Morph(Hydralisk), &[LurkerDen]),
    d(Mutalisk, "Mutalisk", &["mutalisk", "muta"], (100, 100, 24.0), 4, Larva, &[Spire]),
    d(Corruptor, "Corruptor", &["corruptor"], (150, 100, 29.0), 4, Larva, &[Spire]),
    d(BroodLord, "Brood Lord", &["broodlord", "brood lord", "bl"], (150, 150, 24.0), 4, Morph(Corruptor), &[GreaterSpire]),
    d(Infestor, "Infestor", &["infestor"], (100, 150, 36.0), 4, Larva, &[InfestationPit]),
    d(SwarmHost, "Swarm Host", &["swarmhost", "swarm host", "host"], (100, 75, 29.0), 6, Larva, &[InfestationPit]),
    d(Viper, "Viper", &["viper"], (100, 200, 29.0), 6, Larva, &[Hive]),
    d(Ultralisk, "Ultralisk", &["ultralisk", "ultra"], (275, 200, 39.0), 12, Larva, &[UltraliskCavern]),
    d(Overseer, "Overseer", &["overseer"], (50, 50, 12.0), 0, Morph(Overlord), &[Lair]),
    Def { provides: 12, ..d(Hatchery, "Hatchery (expand)", &["hatchery", "hatch", "expansion", "expand", "natural", "base"], (300, 0, 71.0), 0, Worker, &[]) },
    Def { provides: 12, ..d(MacroHatchery, "Hatchery (macro)", &["macro hatchery", "macro hatch", "macrohatch"], (300, 0, 71.0), 0, Worker, &[]) },
    d(Extractor, "Extractor", &["extractor", "gas", "geyser"], (25, 0, 21.0), 0, Worker, &[]),
    d(SpawningPool, "Spawning Pool", &["spawning pool", "spawningpool", "pool"], (200, 0, 46.0), 0, Worker, &[]),
    d(EvolutionChamber, "Evolution Chamber", &["evolution chamber", "evolutionchamber", "evo"], (75, 0, 25.0), 0, Worker, &[]),
    d(RoachWarren, "Roach Warren", &["roach warren", "roachwarren", "warren"], (150, 0, 39.0), 0, Worker, &[SpawningPool]),
    d(BanelingNest, "Baneling Nest", &["baneling nest", "banelingnest", "nest", "bane nest"], (100, 50, 43.0), 0, Worker, &[SpawningPool]),
    d(SpineCrawler, "Spine Crawler", &["spine crawler", "spinecrawler", "spine"], (100, 0, 36.0), 0, Worker, &[SpawningPool]),
    d(SporeCrawler, "Spore Crawler", &["spore crawler", "sporecrawler", "spore"], (75, 0, 21.0), 0, Worker, &[SpawningPool]),
    d(Lair, "Lair", &["lair"], (150, 100, 57.0), 0, TownHall, &[SpawningPool]),
    d(HydraliskDen, "Hydralisk Den", &["hydralisk den", "hydraliskden", "hydra den", "den"], (100, 100, 29.0), 0, Worker, &[Lair]),
    d(LurkerDen, "Lurker Den", &["lurker den", "lurkerden"], (100, 150, 57.0), 0, Worker, &[HydraliskDen]),
    d(InfestationPit, "Infestation Pit", &["infestation pit", "infestationpit", "pit"], (100, 100, 36.0), 0, Worker, &[Lair]),
    d(Spire, "Spire", &["spire"], (200, 200, 71.0), 0, Worker, &[Lair]),
    d(Hive, "Hive", &["hive"], (200, 150, 71.0), 0, TownHall, &[Lair, InfestationPit]),
    d(GreaterSpire, "Greater Spire", &["greater spire", "greaterspire"], (100, 150, 71.0), 0, Building(Spire), &[Hive]),
    d(UltraliskCavern, "Ultralisk Cavern", &["ultralisk cavern", "ultraliskcavern", "cavern"], (150, 200, 46.0), 0, Worker, &[Hive]),
    d(MetabolicBoost, "Metabolic Boost", &["metabolic boost", "metabolicboost", "ling speed", "speed"], (100, 100, 79.0), 0, Building(SpawningPool), &[]),
    d(GlialReconstitution, "Glial Reconstitution", &["glial reconstitution", "glial", "roach speed"], (100, 100, 79.0), 0, Building(RoachWarren), &[Lair]),
    d(CentrifugalHooks, "Centrifugal Hooks", &["centrifugal hooks", "hooks", "bane speed"], (100, 100, 71.0), 0, Building(BanelingNest), &[Lair]),
    d(GroovedSpines, "Grooved Spines", &["grooved spines", "hydra range"], (100, 100, 71.0), 0, Building(HydraliskDen), &[]),
    d(MuscularAugments, "Muscular Augments", &["muscular augments", "hydra speed"], (100, 100, 71.0), 0, Building(HydraliskDen), &[]),
    d(PneumatizedCarapace, "Pneumatized Carapace", &["pneumatized carapace", "overlord speed"], (100, 100, 43.0), 0, TownHall, &[]),
    d(Burrow, "Burrow", &["burrow"], (100, 100, 71.0), 0, TownHall, &[]),
    d(MissileWeapons1, "Missile Attacks 1", &["missile attacks 1", "missile 1", "+1 range", "+1 missile"], (100, 100, 114.0), 0, Building(EvolutionChamber), &[]),
    d(MeleeWeapons1, "Melee Attacks 1", &["melee attacks 1", "melee 1", "+1 melee"], (100, 100, 114.0), 0, Building(EvolutionChamber), &[]),
    d(GroundCarapace1, "Ground Carapace 1", &["ground carapace 1", "carapace 1", "+1 carapace", "+1 armor"], (150, 150, 114.0), 0, Building(EvolutionChamber), &[]),
    // Terran
    d(Scv, "SCV", &["scv", "worker"], (50, 0, 12.0), 2, TownHall, &[]),
    d(Marine, "Marine", &["marine", "rine"], (50, 0, 18.0), 2, Building(Barracks), &[]),
    d(Reaper, "Reaper", &["reaper"], (50, 50, 32.0), 2, Building(Barracks), &[]),
    d(Marauder, "Marauder", &["marauder", "rauder"], (100, 25, 21.0), 4, TechLab(Barracks), &[BarracksTechLab]),
    d(Ghost, "Ghost", &["ghost"], (150, 125, 29.0), 4, TechLab(Barracks), &[BarracksTechLab, GhostAcademy]),
    d(Hellion, "Hellion", &["hellion"], (100, 0, 21.0), 4, Building(Factory), &[]),
    d(Hellbat, "Hellbat", &["hellbat"], (100, 0, 21.0), 4, Building(Factory), &[Armory]),
    d(WidowMine, "Widow Mine", &["widow mine", "widowmine", "mine"], (75, 25, 21.0), 4, Building(Factory), &[]),
    d(Cyclone, "Cyclone", &["cyclone"], (150, 100, 32.0), 6, TechLab(Factory), &[FactoryTechLab]),
    d(SiegeTank, "Siege Tank", &["siege tank", "siegetank", "tank"], (150, 125, 32.0), 6, TechLab(Factory), &[FactoryTechLab]),
    d(Thor, "Thor", &["thor"], (300, 200, 43.0), 12, TechLab(Factory), &[FactoryTechLab, Armory]),
    d(Viking, "Viking", &["viking"], (125, 75, 30.0), 4, Building(Starport), &[]),
    d(Medivac, "Medivac", &["medivac", "medi"], (100, 100, 30.0), 4, Building(Starport), &[]),
    d(Liberator, "Liberator", &["liberator", "lib"], (150, 125, 43.0), 6, Building(Starport), &[]),
    d(Raven, "Raven", &["raven"], (100, 150, 34.0), 4, TechLab(Starport), &[StarportTechLab]),
    d(Banshee, "Banshee", &["banshee"], (150, 100, 43.0), 6, TechLab(Starport), &[StarportTechLab]),
    d(Battlecruiser, "Battlecruiser", &["battlecruiser", "bc"], (400, 300, 64.0), 12, TechLab(Starport), &[StarportTechLab, FusionCore]),
    Def { provides: 30, ..d(CommandCenter, "Command Center (expand)", &["command center", "commandcenter", "cc", "expansion", "expand", "natural", "base"], (400, 0, 71.0), 0, Worker, &[]) },
    d(OrbitalCommand, "Orbital Command", &["orbital command", "orbitalcommand", "orbital", "oc"], (150, 0, 25.0), 0, TownHall, &[Barracks]),
    d(PlanetaryFortress, "Planetary Fortress", &["planetary fortress", "planetaryfortress", "pf"], (150, 150, 36.0), 0, TownHall, &[EngineeringBay]),
    Def { provides: 16, ..d(SupplyDepot, "Supply Depot", &["supply depot", "supplydepot", "depot", "supply"], (100, 0, 21.0), 0, Worker, &[]) },
    d(Refinery, "Refinery", &["refinery", "gas", "geyser"], (75, 0, 21.0), 0, Worker, &[]),
    d(Barracks, "Barracks", &["barracks", "rax"], (150, 0, 46.0), 0, Worker, &[SupplyDepot]),
    d(EngineeringBay, "Engineering Bay", &["engineering bay", "engineeringbay", "ebay"], (125, 0, 25.0), 0, Worker, &[]),
    d(Bunker, "Bunker", &["bunker"], (100, 0, 29.0), 0, Worker, &[Barracks]),
    d(MissileTurret, "Missile Turret", &["missile turret", "missileturret", "turret"], (100, 0, 18.0), 0, Worker, &[EngineeringBay]),
    d(Factory, "Factory", &["factory"], (150, 100, 43.0), 0, Worker, &[Barracks]),
    d(GhostAcademy, "Ghost Academy", &["ghost academy", "ghostacademy", "academy"], (150, 50, 29.0), 0, Worker, &[Barracks]),
    d(Armory, "Armory", &["armory"], (150, 50, 46.0), 0, Worker, &[Factory]),
    d(Starport, "Starport", &["starport"], (150, 100, 36.0), 0, Worker, &[Factory]),
    d(FusionCore, "Fusion Core", &["fusion core", "fusioncore"], (150, 150, 46.0), 0, Worker, &[Starport]),
    d(BarracksTechLab, "Barracks Tech Lab", &["barracks tech lab", "barracks techlab", "rax tech lab", "tech lab", "techlab"], (50, 25, 18.0), 0, AddOn(Barracks), &[]),
    d(BarracksReactor, "Barracks Reactor", &["barracks reactor", "rax reactor", "reactor"], (50, 50, 36.0), 0, AddOn(Barracks), &[]),
    d(FactoryTechLab, "Factory Tech Lab", &["factory tech lab", "factory techlab"], (50, 25, 18.0), 0, AddOn(Factory), &[]),
    d(FactoryReactor, "Factory Reactor", &["factory reactor"], (50, 50, 36.0), 0, AddOn(Factory), &[]),
    d(StarportTechLab, "Starport Tech Lab", &["starport tech lab", "starport techlab"], (50, 25, 18.0), 0, AddOn(Starport), &[]),
    d(StarportReactor, "Starport Reactor", &["starport reactor"], (50, 50, 36.0), 0, AddOn(Starport), &[]),
    d(Stimpack, "Stimpack", &["stimpack", "stim"], (100, 100, 100.0), 0, Building(BarracksTechLab), &[]),
    d(CombatShield, "Combat Shield", &["combat shield", "combatshield", "shield", "shields"], (100, 100, 79.0), 0, Building(BarracksTechLab), &[]),
    d(ConcussiveShells, "Concussive Shells", &["concussive shells", "concussive", "concussive shell"], (50, 50, 43.0), 0, Building(BarracksTechLab), &[]),
    d(InfernalPreIgniter, "Infernal Pre-Igniter", &["infernal pre-igniter", "infernal preigniter", "blue flame"], (100, 100, 79.0), 0, Building(FactoryTechLab), &[]),
    d(CloakingField, "Cloaking Field", &["cloaking field", "banshee cloak", "cloak"], (100, 100, 79.0), 0, Building(StarportTechLab), &[]),
    d(InfantryWeapons1, "Infantry Weapons 1", &["infantry weapons 1", "infantry weapons", "+1 infantry weapons", "+1 attack", "+1 weapons"], (100, 100, 114.0), 0, Building(EngineeringBay), &[]),
    d(InfantryArmor1, "Infantry Armor 1", &["infantry armor 1", "infantry armor", "+1 infantry armor", "+1 armor"], (100, 100, 114.0), 0, Building(EngineeringBay), &[]),
    d(VehicleWeapons1, "Vehicle Weapons 1", &["vehicle weapons 1", "vehicle weapons", "+1 vehicle weapons"], (100, 100, 114.0), 0, Building(Armory), &[]),
    d(ShipWeapons1, "Ship Weapons 1", &["ship weapons 1", "ship weapons", "+1 ship weapons"], (100, 100, 114.0), 0, Building(Armory), &[]),
    // Protoss
    d(Probe, "Probe", &["probe", "worker"], (50, 0, 12.0), 2, TownHall, &[]),
    Def { warp: 20.0, ..d(Zealot, "Zealot", &["zealot", "lot"], (100, 0, 27.0), 4, Building(Gateway), &[]) },
    Def { warp: 23.0, ..d(Stalker, "Stalker", &["stalker"], (125, 50, 27.0), 4, Building(Gateway), &[CyberneticsCore]) },
    Def { warp: 23.0, ..d(Sentry, "Sentry", &["sentry", "sentries"], (50, 100, 23.0), 4, Building(Gateway), &[CyberneticsCore]) },
    Def { warp: 20.0, ..d(Adept, "Adept", &["adept"], (100, 25, 30.0), 4, Building(Gateway), &[CyberneticsCore]) },
    Def { warp: 32.0, ..d(HighTemplar, "High Templar", &["high templar", "hightemplar", "ht", "templar"], (50, 150, 40.0), 4, Building(Gateway), &[TemplarArchives]) },
    Def { warp: 32.0, ..d(DarkTemplar, "Dark Templar", &["dark templar", "darktemplar", "dt"], (125, 125, 40.0), 4, Building(Gateway), &[DarkShrine]) },
    d(Observer, "Observer", &["observer", "obs"], (25, 75, 17.9), 2, Building(RoboticsFacility), &[]),
    d(WarpPrism, "Warp Prism", &["warp prism", "warpprism", "prism"], (250, 0, 36.0), 4, Building(RoboticsFacility), &[]),
    d(Immortal, "Immortal", &["immortal", "immo"], (250, 100, 39.0), 8, Building(RoboticsFacility), &[]),
    d(Colossus, "Colossus", &["colossus", "colossi", "colo"], (300, 200, 54.0), 12, Building(RoboticsFacility), &[RoboticsBay]),
    d(Disruptor, "Disruptor", &["disruptor"], (150, 150, 36.0), 8, Building(RoboticsFacility), &[RoboticsBay]),
    d(Phoenix, "Phoenix", &["phoenix"], (150, 100, 25.0), 4, Building(Stargate), &[]),
    d(Oracle, "Oracle", &["oracle"], (150, 150, 37.0), 6, Building(Stargate), &[]),
    d(VoidRay, "Void Ray", &["void ray", "voidray", "void"], (250, 150, 43.0), 8, Building(Stargate), &[]),
    d(Tempest, "Tempest", &["tempest", "tempests"], (250, 175, 43.0), 8, Building(Stargate), &[FleetBeacon]),
    d(Carrier, "Carrier", &["carrier"], (350, 250, 64.0), 12, Building(Stargate), &[FleetBeacon]),
    Def { provides: 30, ..d(Nexus, "Nexus (expand)", &["nexus", "expansion", "expand", "natural", "base"], (400, 0, 71.0), 0, Worker, &[]) },
    Def { provides: 16, ..d(Pylon, "Pylon", &["pylon", "supply"], (100, 0, 18.0), 0, Worker, &[]) },
    d(Assimilator, "Assimilator", &["assimilator", "assim", "gas", "geyser"], (75, 0, 21.0), 0, Worker, &[]),
    d(Gateway, "Gateway", &["gateway", "gate"], (150, 0, 46.0), 0, Worker, &[Pylon]),
    d(Forge, "Forge", &["forge"], (150, 0, 32.0), 0, Worker, &[Pylon]),
    d(PhotonCannon, "Photon Cannon", &["photon cannon", "photoncannon", "cannon"], (150, 0, 29.0), 0, Worker, &[Pylon, Forge]),
    d(ShieldBattery, "Shield Battery", &["shield battery", "shieldbattery", "battery", "batteries"], (100, 0, 29.0), 0, Worker, &[Pylon, CyberneticsCore]),
    d(CyberneticsCore, "Cybernetics Core", &["cybernetics core", "cyberneticscore", "cyber core", "cyber", "core"], (150, 0, 36.0), 0, Worker, &[Pylon, Gateway]),
    d(TwilightCouncil, "Twilight Council", &["twilight council", "twilightcouncil", "twilight"], (150, 100, 36.0), 0, Worker, &[Pylon, CyberneticsCore]),
    d(Stargate, "Stargate", &["stargate"], (150, 150, 43.0), 0, Worker, &[Pylon, CyberneticsCore]),
    d(RoboticsFacility, "Robotics Facility", &["robotics facility", "roboticsfacility", "robo"], (150, 100, 46.0), 0, Worker, &[Pylon, CyberneticsCore]),
    d(RoboticsBay, "Robotics Bay", &["robotics bay", "roboticsbay", "robo bay"], (150, 150, 46.0), 0, Worker, &[Pylon, RoboticsFacility]),
    d(TemplarArchives, "Templar Archives", &["templar archives", "templararchives", "archives"], (150, 200, 36.0), 0, Worker, &[Pylon, TwilightCouncil]),
    d(DarkShrine, "Dark Shrine", &["dark shrine", "darkshrine", "shrine"], (150, 150, 71.0), 0, Worker, &[Pylon, TwilightCouncil]),
    d(FleetBeacon, "Fleet Beacon", &["fleet beacon", "fleetbeacon", "beacon"], (300, 200, 43.0), 0, Worker, &[Pylon, Stargate]),
    d(WarpGate, "Warp Gate", &["warp gate", "warpgate"], (50, 50, 100.0), 0, Building(CyberneticsCore), &[]),
    d(Charge, "Charge", &["charge"], (100, 100, 100.0), 0, Building(TwilightCouncil), &[]),
    d(Blink, "Blink", &["blink"], (150, 150, 121.0), 0, Building(TwilightCouncil), &[]),
    d(ResonatingGlaives, "Resonating Glaives", &["resonating glaives", "glaives", "adept speed"], (100, 100, 100.0), 0, Building(TwilightCouncil), &[]),
    d(GroundWeapons1, "Ground Weapons 1", &["ground weapons 1", "ground weapons", "+1 ground weapons", "+1 attack", "+1 weapons"], (100, 100, 121.0), 0, Building(Forge), &[]),
    d(GroundArmor1, "Ground Armor 1", &["ground armor 1", "ground armor", "+1 ground armor", "+1 armor"], (100, 100, 121.0), 0, Building(Forge), &[]),
    d(Shields1, "Shields 1", &["shields 1", "+1 shields", "+1 shield"], (150, 150, 121.0), 0, Building(Forge), &[]),
    d(ExtendedThermalLance, "Extended Thermal Lance", &["extended thermal lance", "thermal lance", "colossus range"], (150, 150, 100.0), 0, Building(RoboticsBay), &[]),
    d(PsionicStorm, "Psionic Storm", &["psionic storm", "psionicstorm", "storm"], (200, 200, 79.0), 0, Building(TemplarArchives), &[]),
];

const fn max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}

/// Items in the largest race: the size of every per-item array.
pub const N: usize = max(Race::Zerg.len(), max(Race::Terran.len(), Race::Protoss.len()));

pub fn def(id: Id) -> &'static Def {
    &DEFS[id as usize]
}

pub fn race(id: Id) -> Race {
    if id < Scv {
        Race::Zerg
    } else if id < Probe {
        Race::Terran
    } else {
        Race::Protoss
    }
}

/// Index of `id` within its race, for per-item arrays.
pub fn slot(id: Id) -> usize {
    id as usize - race(id).range().start
}

impl Race {
    pub const ALL: [Race; 3] = [Race::Zerg, Race::Terran, Race::Protoss];

    pub fn name(self) -> &'static str {
        match self {
            Race::Zerg => "Zerg",
            Race::Terran => "Terran",
            Race::Protoss => "Protoss",
        }
    }

    pub fn parse(name: &str) -> Option<Race> {
        Race::ALL.into_iter().find(|r| r.name().eq_ignore_ascii_case(name.trim()))
    }

    const fn range(self) -> Range<usize> {
        match self {
            Race::Zerg => 0..Scv as usize,
            Race::Terran => Scv as usize..Probe as usize,
            Race::Protoss => Probe as usize..DEFS.len(),
        }
    }

    const fn len(self) -> usize {
        let range = self.range();
        range.end - range.start
    }

    pub fn items(self) -> impl Iterator<Item = Id> {
        DEFS[self.range()].iter().map(|d| d.id)
    }

    pub fn worker(self) -> Id {
        match self {
            Race::Zerg => Drone,
            Race::Terran => Scv,
            Race::Protoss => Probe,
        }
    }

    /// The town hall an expansion builds.
    pub fn town_hall(self) -> Id {
        match self {
            Race::Zerg => Hatchery,
            Race::Terran => CommandCenter,
            Race::Protoss => Nexus,
        }
    }

    pub fn supply(self) -> Id {
        match self {
            Race::Zerg => Overlord,
            Race::Terran => SupplyDepot,
            Race::Protoss => Pylon,
        }
    }

    pub fn gas(self) -> Id {
        match self {
            Race::Zerg => Extractor,
            Race::Terran => Refinery,
            Race::Protoss => Assimilator,
        }
    }

    /// The first research item; everything before it from the town hall on
    /// is a structure.
    pub fn first_research(self) -> Id {
        match self {
            Race::Zerg => MetabolicBoost,
            Race::Terran => Stimpack,
            Race::Protoss => WarpGate,
        }
    }
}

/// Items that grow the economy: workers, supply, gas, town halls and what
/// boosts them. The search adds these on its own.
pub fn is_economy(id: Id) -> bool {
    matches!(
        id,
        Drone
            | Overlord
            | Queen
            | Extractor
            | Hatchery
            | MacroHatchery
            | Scv
            | SupplyDepot
            | Refinery
            | CommandCenter
            | OrbitalCommand
            | Probe
            | Pylon
            | Assimilator
            | Nexus
    )
}

/// Items that add production capacity. The search adds extra copies of
/// these on its own when the goal has units they can make.
pub fn is_production(id: Id) -> bool {
    matches!(id, Barracks | Factory | Starport | Gateway | RoboticsFacility | Stargate | WarpGate)
        || matches!(def(id).source, AddOn(_))
}

/// Items the search may add on its own, beyond what the goal strictly requires.
pub fn is_optional(id: Id) -> bool {
    is_economy(id) || is_production(id)
}

pub fn is_town_hall(id: Id) -> bool {
    matches!(id, Hatchery | MacroHatchery | CommandCenter | Nexus)
}

/// Town hall morphs that each command center can take only once.
pub fn is_town_hall_upgrade(id: Id) -> bool {
    matches!(id, OrbitalCommand | PlanetaryFortress)
}

/// The tech lab and reactor a structure can build.
pub fn addons(building: Id) -> Option<(Id, Id)> {
    match building {
        Barracks => Some((BarracksTechLab, BarracksReactor)),
        Factory => Some((FactoryTechLab, FactoryReactor)),
        Starport => Some((StarportTechLab, StarportReactor)),
        _ => None,
    }
}

/// The structure `id` is trained or researched at, if it occupies one.
pub fn producer(id: Id) -> Option<Id> {
    match def(id).source {
        Building(b) | TechLab(b) => Some(b),
        _ => None,
    }
}

/// Whether finished `id`s train or research anything, so the simulation has
/// to track when each one is busy.
pub fn is_producer(id: Id) -> bool {
    is_town_hall(id) || race(id).items().any(|other| producer(other) == Some(id))
}

/// Every race's items whose names match, after stripping a plural ending.
/// Each race has at most one match.
pub fn lookup(name: &str) -> Vec<Id> {
    let name = name.trim().to_lowercase();
    let find = |n: &str| DEFS.iter().filter(|d| d.aliases.contains(&n)).map(|d| d.id).collect::<Vec<_>>();
    [Some(name.as_str()), name.strip_suffix("es"), name.strip_suffix('s')]
        .into_iter()
        .flatten()
        .map(find)
        .find(|ids| !ids.is_empty())
        .unwrap_or_default()
}

// Game constants (real seconds, Faster speed).

/// Minerals per second for each of the first two workers on a patch.
pub const MINERAL_RATE: f64 = 0.94;
/// Minerals per second for a third worker on a patch (heavily diminished).
pub const MINERAL_RATE_THIRD: f64 = 0.40;
pub const PATCHES_PER_BASE: u32 = 8;
/// Gas per second per worker; three workers saturate a geyser.
pub const GAS_RATE: f64 = 0.89;
pub const WORKERS_PER_GEYSER: u32 = 3;
pub const GEYSERS_PER_BASE: u16 = 2;
pub const START_WORKERS: u16 = 12;
pub const START_MINERALS: f64 = 50.0;
pub const MAX_SUPPLY: u32 = 400;
/// Energy regeneration of queens, orbital commands and nexuses.
pub const ENERGY_REGEN: f64 = 0.7875;
pub const MAX_ENERGY: f64 = 200.0;

// Zerg
pub const LARVA_INTERVAL: f64 = 11.0;
pub const NATURAL_LARVA_CAP: u8 = 3;
pub const MAX_LARVA: u8 = 19;
pub const START_LARVA: u8 = 3;
pub const INJECT_TIME: f64 = 29.0;
pub const INJECT_LARVA: u8 = 3;
pub const INJECT_ENERGY: f64 = 25.0;
pub const QUEEN_START_ENERGY: f64 = 25.0;
/// Larvae on a freshly completed hatchery.
pub const NEW_HATCH_LARVA: u8 = 1;

// Terran
pub const ORBITAL_START_ENERGY: f64 = 50.0;
pub const MULE_ENERGY: f64 = 50.0;
pub const MULE_DURATION: f64 = 64.0;
/// Minerals one MULE mines over its lifetime.
pub const MULE_MINERALS: f64 = 225.0;

// Protoss
pub const NEXUS_START_ENERGY: f64 = 50.0;
pub const CHRONO_ENERGY: f64 = 50.0;
pub const CHRONO_DURATION: f64 = 20.0;
/// Production speed under Chrono Boost.
pub const CHRONO_SPEED: f64 = 1.5;
/// Warping in a unit at a gateway or nexus power field.
pub const WARP_IN_TIME: f64 = 3.6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_indexed_by_id() {
        for (i, d) in DEFS.iter().enumerate() {
            assert_eq!(d.id as usize, i, "{} is out of order", d.name);
        }
    }

    #[test]
    fn races_are_contiguous_and_slots_round_trip() {
        for race in Race::ALL {
            for (i, id) in race.items().enumerate() {
                assert_eq!(super::race(id), race);
                assert_eq!(slot(id), i);
            }
            assert!(race.items().count() <= N);
        }
    }

    #[test]
    fn aliases_are_unique_within_a_race_and_first_ones_globally() {
        for a in &DEFS {
            for b in DEFS.iter().filter(|b| b.id != a.id) {
                if race(a.id) == race(b.id) {
                    assert!(a.aliases.iter().all(|n| !b.aliases.contains(n)), "{} and {} share a name", a.name, b.name);
                }
                assert!(!b.aliases.contains(&a.aliases[0]), "{} is ambiguous", a.aliases[0]);
            }
        }
    }

    #[test]
    fn requirements_stay_within_the_race() {
        for d in &DEFS {
            let source = match d.source {
                Morph(s) | Building(s) | TechLab(s) | AddOn(s) => Some(s),
                _ => None,
            };
            for &r in d.requires.iter().chain(source.iter()) {
                assert_eq!(race(r), race(d.id), "{} requires {:?}", d.name, r);
            }
        }
    }

    #[test]
    fn lookup_handles_plurals_and_aliases() {
        assert_eq!(lookup("roaches"), [Roach]);
        assert_eq!(lookup("Lings"), [Zergling]);
        assert_eq!(lookup("pool"), [SpawningPool]);
        assert_eq!(lookup("hydras"), [Hydralisk]);
        assert_eq!(lookup("marines"), [Marine]);
        assert_eq!(lookup("phoenixes"), [Phoenix]);
        assert_eq!(lookup("gas"), [Extractor, Refinery, Assimilator]);
        assert_eq!(lookup("nope"), []);
    }
}
