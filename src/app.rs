//! The desktop app. The user picks a goal and tunes the model, the search runs
//! in the background, and the fastest build is drawn as a timeline of when each
//! action is in production.

use crate::build::{self, Build, Kind};
use crate::data::{self, Id, Race};
use crate::plan::{self, Plan};
use crate::progress::{self, Progress, Snapshot};
use crate::search;
use crate::sim::{Config, clock, ticks};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt, Theme, ThemeMode, TitleBar, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(80);
const SIDEBAR_WIDTH: Pixels = px(340.);
const STATE_BUDGETS: [usize; 6] = [250_000, 500_000, 1_000_000, 2_000_000, 4_000_000, 8_000_000];

fn presets(race: Race) -> [&'static str; 6] {
    match race {
        Race::Zerg => [
            "10 roaches",
            "roach=8, ravager=2",
            "20 lings and ling speed",
            "5 mutas",
            "glial",
            "8 hydras and hydra range",
        ],
        Race::Terran => [
            "10 marines",
            "6 marines and stim",
            "4 marauders, 2 medivacs",
            "2 tanks",
            "4 hellions",
            "banshee and cloak",
        ],
        Race::Protoss => [
            "4 stalkers",
            "4 stalkers and blink",
            "8 zealots and charge",
            "2 immortals",
            "2 oracles",
            "2 colossi",
        ],
    }
}

pub fn run() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        let options = WindowOptions {
            window_min_size: Some(size(px(980.), px(640.))),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1320.), px(860.)), cx))),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| BuildOptimizer::new(window, cx)))
            .expect("failed to open window");
        cx.activate(true);
    });
}

actions!(sc2_build_optimizer, [Quit]);

fn kind_color(kind: Kind, cx: &App) -> Hsla {
    let c = &cx.theme().colors;
    match kind {
        Kind::Economy => c.cyan,
        Kind::Army => c.red,
        Kind::Structure => c.yellow,
        Kind::Research => c.magenta,
    }
}

/// A finished search, as the main area shows it.
struct Finished {
    goal: String,
    build: Build,
    elapsed: Duration,
}

struct Running {
    snapshot: Arc<Mutex<Snapshot>>,
    cancel: Arc<AtomicBool>,
    started: Instant,
}

struct BuildOptimizer {
    input: Entity<InputState>,
    /// The race the palette and presets show. It follows the goal, and
    /// decides names any race could mean, like "gas".
    race: Race,
    /// The goal as parsed from the input, or why it does not parse.
    goal: Result<Vec<(Id, u16)>, String>,
    cfg: Config,
    /// Index into `STATE_BUDGETS`.
    budget: usize,
    running: Option<Running>,
    outcome: Option<Result<Finished, String>>,
    hovered: Option<usize>,
}

impl BuildOptimizer {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let first = presets(Race::Zerg)[0];
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. 8 roaches and glial").default_value(first));
        cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| match event {
            InputEvent::Change => {
                this.parse(&input.read(cx).value());
                cx.notify();
            }
            InputEvent::PressEnter { .. } => this.start(cx),
            _ => {}
        })
        .detach();
        BuildOptimizer {
            input,
            race: Race::Zerg,
            goal: plan::parse_goal(first, Race::Zerg),
            cfg: Config {
                expand_travel: 12.0,
                max_expansions: 1,
                max_macro_hatcheries: 0,
                max_extractors: 2,
                max_queens: 2,
                max_production: 3,
                horizon: ticks(20.0 * 60.0),
            },
            budget: 2,
            running: None,
            outcome: None,
            hovered: None,
        }
    }

    fn set_goal(&mut self, goal: Vec<(Id, u16)>, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_value(build::goal_text(&goal), window, cx));
        self.goal = if goal.is_empty() { Err("empty goal".into()) } else { Ok(goal) };
        cx.notify();
    }

    fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_value(text.to_string(), window, cx));
        self.parse(text);
        cx.notify();
    }

    fn parse(&mut self, text: &str) {
        self.goal = plan::parse_goal(text, self.race);
        if let Ok(goal) = &self.goal {
            self.race = data::race(goal[0].0);
        }
    }

    /// Switches race, starting over from its first preset.
    fn set_race(&mut self, race: Race, window: &mut Window, cx: &mut Context<Self>) {
        if race != self.race {
            self.race = race;
            self.set_text(presets(race)[0], window, cx);
        }
    }

    fn adjust(&mut self, id: Id, actions: i32, window: &mut Window, cx: &mut Context<Self>) {
        let mut goal = self.goal.clone().unwrap_or_default();
        build::adjust(&mut goal, id, actions);
        self.set_goal(goal, window, cx);
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let Ok(goal) = self.goal.clone() else { return };
        self.stop();
        let plan = Plan::new(&goal);
        if let Err(e) = plan.check() {
            self.outcome = Some(Err(format!("The {e}.")));
            cx.notify();
            return;
        }

        let running = Running { snapshot: Arc::default(), cancel: Arc::default(), started: Instant::now() };
        let mut progress = Progress::shared(running.snapshot.clone(), running.cancel.clone());
        let cancel = running.cancel.clone();
        let (cfg, budget) = (self.cfg, STATE_BUDGETS[self.budget]);
        let started = running.started;
        let searching = cx.background_spawn(async move {
            let sol = search::search(&plan, &cfg, 2000, budget, &mut progress);
            sol.map(|sol| Finished { goal: Plan::describe(&goal), build: Build::new(&sol), elapsed: started.elapsed() })
            .ok_or_else(|| "No build order reaches this goal within 20 minutes.".to_string())
        });
        self.running = Some(running);
        self.hovered = None;

        let current = cancel.clone();
        cx.spawn(async move |this, cx| {
            let outcome = searching.await;
            this.update(cx, |this, cx| {
                if this.running.as_ref().is_some_and(|r| Arc::ptr_eq(&r.cancel, &current)) {
                    this.running = None;
                    this.outcome = Some(outcome);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();

        // Redraws the progress until this search ends or another replaces it.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let alive = this.update(cx, |this, cx| {
                    cx.notify();
                    this.running.as_ref().is_some_and(|r| Arc::ptr_eq(&r.cancel, &cancel))
                });
                if !alive.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Render for BuildOptimizer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.render_sidebar(cx))
                    .child(self.render_main(cx)),
            )
    }
}

// Sidebar: goal, unit palette and model settings.
impl BuildOptimizer {
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = cx.theme().is_dark();
        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .child(logo(self.race, cx))
                        .child(div().text_sm().font_semibold().child("Build Optimizer"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{} · LotV 5.0", self.race.name())),
                        ),
                )
                .child(
                    Button::new("theme")
                        .ghost()
                        .small()
                        .icon(if dark { IconName::Sun } else { IconName::Moon })
                        .tooltip(if dark { "Light mode" } else { "Dark mode" })
                        .on_click(move |_, window, cx| {
                            let mode = if dark { ThemeMode::Light } else { ThemeMode::Dark };
                            Theme::change(mode, Some(window), cx);
                        }),
                ),
        )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let running = self.running.is_some();
        let can_run = self.goal.is_ok();

        let run_button = if running {
            Button::new("stop")
                .danger()
                .w_full()
                .icon(IconName::Square)
                .label("Stop search")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.stop();
                    cx.notify();
                }))
        } else {
            Button::new("run")
                .primary()
                .w_full()
                .icon(IconName::Play)
                .label("Find fastest build")
                .disabled(!can_run)
                .on_click(cx.listener(|this, _, _, cx| this.start(cx)))
        };

        v_flex()
            .w(SIDEBAR_WIDTH)
            .flex_shrink_0()
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                v_flex()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .p_4()
                            .gap_5()
                            .child(self.render_goal(cx))
                            .child(self.render_palette(cx))
                            .child(self.render_settings(cx)),
                    ),
            )
            .child(
                v_flex()
                    .p_4()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.sidebar_border)
                    .child(run_button)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .text_center()
                            .child("Press Enter in the goal field to search"),
                    ),
            )
    }

    fn render_goal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let tags = match &self.goal {
            Ok(goal) => h_flex()
                .flex_wrap()
                .gap_1p5()
                .children(goal.iter().map(|&(id, n)| {
                    let color = kind_color(Kind::of(id), cx);
                    h_flex()
                        .id(SharedString::from(format!("goal-{id:?}")))
                        .gap_1()
                        .pl_2()
                        .pr_1()
                        .py_0p5()
                        .rounded(theme.radius)
                        .bg(color.opacity(0.14))
                        .border_1()
                        .border_color(color.opacity(0.35))
                        .text_xs()
                        .child(div().font_semibold().child(n.to_string()))
                        .child(data::def(id).name)
                        .child(
                            Button::new(SharedString::from(format!("goal-rm-{id:?}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let goal = this.goal.clone().unwrap_or_default();
                                    this.set_goal(goal.into_iter().filter(|(g, _)| *g != id).collect(), window, cx);
                                })),
                        )
                }))
                .into_any_element(),
            Err(err) => h_flex()
                .gap_1p5()
                .text_xs()
                .text_color(theme.danger)
                .child(Icon::new(IconName::CircleAlert).xsmall())
                .child(capitalize(err))
                .into_any_element(),
        };

        let races = h_flex().gap_1().children(Race::ALL.into_iter().enumerate().map(|(i, race)| {
            let button = Button::new(("race", i)).small().flex_1().label(race.name());
            let button = if race == self.race { button.primary() } else { button.outline() };
            button.on_click(cx.listener(move |this, _, window, cx| this.set_race(race, window, cx)))
        }));

        v_flex()
            .gap_2()
            .child(section_label("Goal", cx))
            .child(races)
            .child(Input::new(&self.input).prefix(Icon::new(IconName::Search).small()).cleanable(true))
            .child(tags)
            .child(
                h_flex().flex_wrap().gap_1().pt_1().children(presets(self.race).into_iter().enumerate().map(|(i, preset)| {
                    Button::new(("preset", i))
                        .outline()
                        .xsmall()
                        .label(preset)
                        .on_click(cx.listener(move |this, _, window, cx| this.set_text(preset, window, cx)))
                })),
            )
    }

    fn render_palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = [
            ("Units", Kind::Army),
            ("Economy", Kind::Economy),
            ("Structures", Kind::Structure),
            ("Research", Kind::Research),
        ];
        let counts = self.goal.clone().unwrap_or_default();
        v_flex().gap_3().child(section_label("Add to goal", cx)).children(groups.into_iter().map(|(title, kind)| {
            let color = kind_color(kind, cx);
            v_flex()
                .gap_1p5()
                .child(
                    h_flex()
                        .gap_1p5()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().size_1p5().rounded_full().bg(color))
                        .child(title),
                )
                .child(h_flex().flex_wrap().gap_1().children(self.race.items().filter(|&id| Kind::of(id) == kind).map(|id| {
                    let def = data::def(id);
                    let have = counts.iter().find(|(g, _)| *g == id).map(|g| g.1);
                    let theme = cx.theme();
                    h_flex()
                        .id(SharedString::from(format!("add-{id:?}")))
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded(theme.radius)
                        .border_1()
                        .text_xs()
                        .cursor_pointer()
                        .map(|chip| match have {
                            Some(_) => chip.bg(color.opacity(0.18)).border_color(color.opacity(0.5)),
                            None => chip.bg(theme.background).border_color(theme.border),
                        })
                        .hover(|chip| chip.border_color(color))
                        .child(def.name)
                        .when_some(have, |chip, n| chip.child(div().font_semibold().text_color(color).child(format!("×{n}"))))
                        .tooltip(move |window, cx| {
                            let gas = if def.gas > 0 { format!(" · {} gas", def.gas) } else { String::new() };
                            Tooltip::new(format!("{} minerals{gas} · {}s. Right-click removes one.", def.minerals, def.time))
                                .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| this.adjust(id, 1, window, cx)))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, _, window, cx| this.adjust(id, -1, window, cx)),
                        )
                })))
        }))
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.cfg;
        let zerg = self.race == Race::Zerg;
        v_flex()
            .gap_1()
            .child(section_label("Model", cx))
            .child(stepper(
                "travel",
                "Worker walk to natural",
                format!("{}s", cfg.expand_travel),
                cfg.expand_travel > 0.0,
                cfg.expand_travel < 40.0,
                |this, d| this.cfg.expand_travel = (this.cfg.expand_travel + d as f64).clamp(0.0, 40.0),
                cx,
            ))
            .child(count_stepper("expansions", "Expansions", cfg.max_expansions, 0, 3, |c| &mut c.max_expansions, cx))
            .child(count_stepper("extractors", "Gas buildings", cfg.max_extractors, 0, 6, |c| &mut c.max_extractors, cx))
            .when(zerg, |settings| {
                settings
                    .child(count_stepper(
                        "macro",
                        "Macro hatcheries",
                        cfg.max_macro_hatcheries,
                        0,
                        3,
                        |c| &mut c.max_macro_hatcheries,
                        cx,
                    ))
                    .child(count_stepper("queens", "Queens for injects", cfg.max_queens, 0, 4, |c| &mut c.max_queens, cx))
            })
            .when(!zerg, |settings| {
                settings.child(count_stepper(
                    "production",
                    "Production per type",
                    cfg.max_production,
                    1,
                    6,
                    |c| &mut c.max_production,
                    cx,
                ))
            })
            .child(stepper(
                "budget",
                "Search budget",
                format!("{} states", short_count(STATE_BUDGETS[self.budget])),
                self.budget > 0,
                self.budget + 1 < STATE_BUDGETS.len(),
                |this, d| this.budget = (this.budget as i32 + d) as usize,
                cx,
            ))
    }
}

// Main area: progress, results and the timeline.
impl BuildOptimizer {
    fn render_main(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if let Some(running) = &self.running {
            self.render_searching(running, cx).into_any_element()
        } else {
            match &self.outcome {
                None => empty_state(
                    IconName::Map,
                    "Pick a goal",
                    "Build Optimizer searches every build order the model allows and shows the fastest one.",
                    cx,
                )
                .into_any_element(),
                Some(Err(err)) => empty_state(IconName::TriangleAlert, "No build found", err, cx).into_any_element(),
                Some(Ok(build)) => self.render_build(build, cx).into_any_element(),
            }
        };
        div().flex_1().min_w_0().h_full().child(content)
    }

    fn render_searching(&self, running: &Running, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = *running.snapshot.lock().unwrap();
        let theme = cx.theme();
        let pass = if snap.pass.is_empty() { "Greedy bound" } else { snap.pass };
        let passes = ["Quick pass", "Rough search", "Exact search"];
        let current = passes.iter().position(|p| *p == pass);

        v_flex().size_full().items_center().justify_center().gap_6().child(
            v_flex()
                .w(px(440.))
                .p_6()
                .gap_5()
                .rounded(theme.radius_lg)
                .bg(theme.secondary)
                .border_1()
                .border_color(theme.border)
                .child(
                    h_flex()
                        .gap_3()
                        .child(Spinner::new().color(theme.primary).large())
                        .child(
                            v_flex()
                                .child(div().text_lg().font_semibold().child("Searching…"))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{:.1}s elapsed", running.started.elapsed().as_secs_f32())),
                                ),
                        ),
                )
                .child(h_flex().gap_2().children(passes.iter().enumerate().map(|(i, p)| {
                    let state = match current {
                        Some(c) if i < c => 2,
                        Some(c) if i == c => 1,
                        _ => 0,
                    };
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .child(div().h_1().rounded_full().bg(match state {
                            2 => theme.primary,
                            1 => theme.primary.opacity(0.55),
                            _ => theme.border,
                        }))
                        .child(
                            div()
                                .text_xs()
                                .text_color(if state > 0 { theme.foreground } else { theme.muted_foreground })
                                .child(*p),
                        )
                })))
                .child(
                    h_flex()
                        .gap_3()
                        .child(stat("States explored", progress::group(snap.explored), None, cx))
                        .child(stat(
                            "Best so far",
                            snap.best.map_or_else(|| "—".to_string(), clock),
                            Some("game time"),
                            cx,
                        )),
                ),
        )
    }

    fn render_build(&self, finished: &Finished, cx: &mut Context<Self>) -> impl IntoElement {
        let build = &finished.build;
        let theme = cx.theme();
        let verdict = if build.proven {
            Tag::success().child(h_flex().gap_1().child(Icon::new(IconName::CircleCheck).xsmall()).child("Proven fastest"))
        } else {
            Tag::warning()
                .child(h_flex().gap_1().child(Icon::new(IconName::TriangleAlert).xsmall()).child("Budget hit · best found"))
        };
        v_flex()
            .size_full()
            .p_6()
            .gap_5()
            .child(
                h_flex()
                    .justify_between()
                    .items_end()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().text_color(theme.muted_foreground).child("FASTEST BUILD FOR"))
                            .child(div().text_2xl().font_bold().child(finished.goal.clone())),
                    )
                    .child(verdict),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(stat("Done at", clock(build.finish), Some("game time"), cx))
                    .child(stat("Actions", build.steps.len().to_string(), None, cx))
                    .child(stat("Peak supply", build.peak_supply().to_string(), None, cx))
                    .child(stat("States explored", progress::group(build.explored), None, cx))
                    .child(stat("Search took", format!("{:.1}s", finished.elapsed.as_secs_f32()), None, cx)),
            )
            .child(self.render_timeline(build, cx))
    }

    fn render_timeline(&self, build: &Build, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let at = |tick: u32| relative(build.fraction(tick));
        let marks = build.marks();
        let mono = theme.mono_font_family.clone();

        let grid = {
            let marks = marks.clone();
            let border = theme.border;
            let finish_color = theme.primary;
            move || {
                div()
                    .absolute()
                    .inset_0()
                    .children(marks.iter().map(|&t| div().absolute().top_0().bottom_0().left(at(t)).w_px().bg(border.opacity(0.5))))
                    .child(div().absolute().top_0().bottom_0().left(at(build.finish)).w(px(2.)).bg(finish_color))
            }
        };

        let header = h_flex()
            .h_8()
            .px_3()
            .text_xs()
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(px(56.)).child("Start"))
            .child(div().w(px(56.)).child("Supply"))
            .child(div().w(px(180.)).child("Action"))
            .child(
                div().flex_1().h_full().relative().children(marks.iter().map(|&t| {
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(at(t))
                        .flex()
                        .items_center()
                        .pl_1()
                        .font_family(mono.clone())
                        .child(clock(t))
                })),
            );

        let rows = build.steps.iter().enumerate().map(|(i, step)| {
            let def = data::def(step.id);
            let kind = Kind::of(step.id);
            let color = kind_color(kind, cx);
            let hovered = self.hovered == Some(i);
            h_flex()
                .id(("step", i))
                .h(px(30.))
                .px_3()
                .text_sm()
                .when(i % 2 == 1, |row| row.bg(theme.table_even))
                .when(hovered, |row| row.bg(theme.table_hover))
                .on_hover(cx.listener(move |this, hover: &bool, _, cx| {
                    if *hover {
                        this.hovered = Some(i);
                    } else if this.hovered == Some(i) {
                        this.hovered = None;
                    }
                    cx.notify();
                }))
                .child(div().w(px(56.)).font_family(mono.clone()).child(clock(step.start)))
                .child(div().w(px(56.)).font_family(mono.clone()).text_color(theme.muted_foreground).child(step.supply.to_string()))
                .child(
                    h_flex()
                        .w(px(180.))
                        .gap_2()
                        .overflow_hidden()
                        .child(div().flex_shrink_0().size_2().rounded_full().bg(color))
                        .child(div().truncate().child(def.name)),
                )
                .child(
                    div().flex_1().h_full().relative().child(grid()).child(
                        div()
                            .absolute()
                            .top(px(7.))
                            .bottom(px(7.))
                            .left(at(step.start))
                            .w(at(step.end - step.start))
                            .min_w(px(3.))
                            .rounded(px(4.))
                            .bg(color.opacity(if hovered { 0.85 } else { 0.55 }))
                            .border_1()
                            .border_color(color)
                            .when(hovered, |bar| {
                                bar.child(
                                    div()
                                        .absolute()
                                        .left_full()
                                        .top_0()
                                        .bottom_0()
                                        .ml_1p5()
                                        .flex()
                                        .items_center()
                                        .text_xs()
                                        .whitespace_nowrap()
                                        .font_family(mono.clone())
                                        .text_color(theme.foreground)
                                        .child(format!("{} → {}", clock(step.start), clock(step.end))),
                                )
                            }),
                    ),
                )
        });

        let legend = h_flex().gap_4().text_xs().text_color(theme.muted_foreground).children(
            Kind::ALL.map(|k| {
                h_flex().gap_1p5().child(div().size_2().rounded_full().bg(kind_color(k, cx))).child(k.label())
            }),
        );

        v_flex()
            .flex_1()
            .min_h_0()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary.opacity(0.4))
            .overflow_hidden()
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_semibold().child("Timeline"))
                    .child(legend),
            )
            .child(header)
            .child(div().id("timeline-rows").flex_1().min_h_0().overflow_y_scrollbar().children(rows))
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().w(px(10.)).h(px(2.)).bg(theme.primary))
                    .child(format!("Goal complete at {} game time", clock(build.finish))),
            )
    }
}

fn logo(race: Race, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .size(px(20.))
        .rounded(px(6.))
        .bg(linear_gradient(
            135.,
            linear_color_stop(theme.magenta, 0.),
            linear_color_stop(theme.red, 1.),
        ))
        .flex()
        .items_center()
        .justify_center()
        .text_color(white())
        .text_xs()
        .font_bold()
        .child(&race.name()[..1])
}

fn section_label(text: &'static str, cx: &App) -> impl IntoElement {
    div().text_xs().font_semibold().text_color(cx.theme().muted_foreground).child(text.to_uppercase())
}

fn stat(label: &'static str, value: String, unit: Option<&'static str>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w_0()
        .px_4()
        .py_3()
        .gap_1()
        .rounded(theme.radius_lg)
        .bg(theme.secondary)
        .border_1()
        .border_color(theme.border)
        .child(div().text_xs().text_color(theme.muted_foreground).child(label))
        .child(
            h_flex()
                .items_baseline()
                .gap_1()
                .child(div().text_xl().font_semibold().font_family(theme.mono_font_family.clone()).child(value))
                .when_some(unit, |row, unit| row.child(div().text_xs().text_color(theme.muted_foreground).child(unit))),
        )
}

fn empty_state(icon: IconName, title: &'static str, body: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(
            div()
                .size(px(56.))
                .rounded_full()
                .bg(theme.secondary)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).large().text_color(theme.muted_foreground)),
        )
        .child(div().text_lg().font_semibold().child(title))
        .child(div().max_w(px(380.)).text_sm().text_center().text_color(theme.muted_foreground).child(body.to_string()))
}

/// A labelled −/+ control for one setting. `change` gets −1 or +1.
fn stepper(
    id: &'static str,
    label: &'static str,
    value: String,
    can_dec: bool,
    can_inc: bool,
    change: impl Fn(&mut BuildOptimizer, i32) + Clone + 'static,
    cx: &mut Context<BuildOptimizer>,
) -> impl IntoElement {
    let dec = change.clone();
    h_flex()
        .h_8()
        .justify_between()
        .text_sm()
        .child(label)
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new((id, 0usize))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Minus)
                        .disabled(!can_dec)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            dec(this, -1);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .min_w(px(84.))
                        .text_center()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .child(value),
                )
                .child(
                    Button::new((id, 1usize))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Plus)
                        .disabled(!can_inc)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            change(this, 1);
                            cx.notify();
                        })),
                ),
        )
}

fn count_stepper(
    id: &'static str,
    label: &'static str,
    value: u16,
    min: u16,
    max: u16,
    field: fn(&mut Config) -> &mut u16,
    cx: &mut Context<BuildOptimizer>,
) -> impl IntoElement {
    stepper(
        id,
        label,
        value.to_string(),
        value > min,
        value < max,
        move |this, d| {
            let v = field(&mut this.cfg);
            *v = (*v as i32 + d).clamp(min as i32, max as i32) as u16;
        },
        cx,
    )
}

fn short_count(n: usize) -> String {
    if n >= 1_000_000 { format!("{}M", n / 1_000_000) } else { format!("{}k", n / 1000) }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

