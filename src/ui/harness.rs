//! Opt-in native GUI regression driver. It feeds egui raw pointer/keyboard/drop
//! events into the real window, targets rectangles observed from the last UI
//! pass, and asserts model/output state. No alternative editing implementation.
use super::*;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
enum Target {
    Named(String),
    Clip(usize, usize),
    ClipAt(usize, f64),
    Ruler(f64),
    Fraction(String, f32),
    LaneAt(usize, f64),
    Edge(usize, usize, f64),
    LeftEdge(usize, usize, f64),
    Corner(usize, usize, bool, f64),
}
#[derive(Clone, Debug)]
enum Step {
    Drop,
    Click(Target),
    Drag(Target, Target),
    Remember,
    Text(String),
    Key(Key, Modifiers),
    Wait(&'static str),
    Assert(&'static str),
    RenderPath,
    CloseRender,
    Finish,
    Crash,
}
pub(super) struct Harness {
    pub root: PathBuf,
    steps: VecDeque<Step>,
    events: VecDeque<egui::Event>,
    started: Instant,
    last_step: Instant,
    index: usize,
    cooldown: u8,
    stage: String,
    done: bool,
    report: Vec<String>,
    baseline: Option<Project>,
    last_report: Instant,
}
fn key(k: Key, m: Modifiers) -> Step {
    Step::Key(k, m)
}
fn click(s: &str) -> Step {
    Step::Click(Target::Named(s.into()))
}
fn prop(steps: &mut VecDeque<Step>, label: &str, value: &str) {
    steps.push_back(click(label));
    steps.push_back(key(Key::A, Modifiers::COMMAND));
    steps.push_back(Step::Text(value.into()));
    steps.push_back(key(Key::Enter, Modifiers::NONE));
}
impl Harness {
    pub fn new(stage: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-output");
        let mut steps = VecDeque::new();
        if stage == "qol" {
            steps.push_back(Step::Drop);
            steps.push_back(Step::Wait("imported"));
            for _ in 0..6 {
                steps.push_back(key(
                    Key::ArrowUp,
                    Modifiers {
                        alt: true,
                        ..Modifiers::NONE
                    },
                ));
            }
            steps.push_back(click("media:gameplay-1.mp4"));
            steps.push_back(key(Key::Enter, Modifiers::COMMAND));
            prop(&mut steps, "Duration (s)", "6");
            steps.push_back(click("fit"));
            steps.push_back(Step::Wait("preview"));
            steps.push_back(Step::Assert("separate-linked"));
            steps.push_back(key(Key::L, Modifiers::COMMAND));
            steps.push_back(Step::Wait("loudness"));
            steps.push_back(Step::Assert("linked-loudness"));
            steps.push_back(Step::Click(Target::ClipAt(0, 2.)));
            steps.push_back(key(Key::S, Modifiers::NONE));
            steps.push_back(Step::Assert("one-click-split"));
            steps.push_back(key(Key::Z, Modifiers::COMMAND));
            steps.push_back(Step::Drag(Target::Edge(0, 0, 0.), Target::Edge(0, 0, -2.)));
            steps.push_back(Step::Assert("trim-four"));
            steps.push_back(Step::Drag(Target::Edge(0, 0, 0.), Target::Edge(0, 0, 1.)));
            steps.push_back(Step::Assert("extend-five"));
            steps.push_back(Step::Drag(
                Target::LeftEdge(0, 0, 0.),
                Target::LeftEdge(0, 0, 1.),
            ));
            steps.push_back(Step::Assert("left-trim"));
            steps.push_back(Step::Drag(
                Target::Corner(0, 0, true, 0.),
                Target::Corner(0, 0, true, 0.5),
            ));
            steps.push_back(Step::Assert("video-fade"));
            steps.push_back(Step::Drag(
                Target::Corner(1, 0, false, 0.),
                Target::Corner(1, 0, false, -0.5),
            ));
            steps.push_back(Step::Assert("audio-fade"));
            steps.push_back(Step::Click(Target::Clip(1, 0)));
            steps.push_back(click("unlink"));
            steps.push_back(Step::Drag(Target::Clip(1, 0), Target::LaneAt(2, 2.)));
            steps.push_back(Step::Assert("unlink-move"));
            steps.push_back(key(Key::Z, Modifiers::COMMAND));
            steps.push_back(key(Key::Z, Modifiers::COMMAND));
            steps.push_back(Step::Wait("preview"));
            steps.push_back(key(Key::Space, Modifiers::NONE));
            steps.push_back(Step::Wait("played"));
            steps.push_back(key(Key::Space, Modifiers::NONE));
        } else if stage == "crash-write" {
            steps.push_back(Step::Wait("opened"));
            steps.push_back(Step::Click(Target::Clip(1, 0)));
            prop(&mut steps, "Gain (dB)", "-13");
            steps.push_back(Step::Wait("autosave"));
            steps.push_back(Step::Crash);
        } else if stage == "crash-read" {
            steps.push_back(click("recover"));
            steps.push_back(Step::Wait("opened"));
            steps.push_back(Step::Assert("recovered"));
            steps.push_back(Step::Finish);
        } else if stage == "1" {
            steps.push_back(Step::Drop);
            steps.push_back(Step::Wait("imported"));
            for _ in 0..6 {
                steps.push_back(key(
                    Key::ArrowUp,
                    Modifiers {
                        alt: true,
                        ..Modifiers::NONE
                    },
                ));
            }
            for i in 1..=3 {
                if i > 1 {
                    steps.push_back(key(
                        Key::ArrowDown,
                        Modifiers {
                            alt: true,
                            ..Modifiers::NONE
                        },
                    ));
                }
                steps.push_back(Step::Click(Target::Named(format!(
                    "media:gameplay-{i}.mp4"
                ))));
                steps.push_back(key(Key::Enter, Modifiers::COMMAND));
                prop(&mut steps, "Duration (s)", "80");
            }
            steps.push_back(click("fit"));
            steps.push_back(Step::Assert("assembled"));
            for n in 0..10 {
                let t = 5. + n as f64 * 5.;
                steps.push_back(Step::Click(Target::ClipAt(0, t + 0.1)));
                steps.push_back(Step::Click(Target::Ruler(t)));
                steps.push_back(key(Key::S, Modifiers::NONE));
                steps.push_back(Step::Click(Target::ClipAt(0, t + 0.5)));
                steps.push_back(Step::Click(Target::Ruler(t + 1.)));
                steps.push_back(key(Key::S, Modifiers::NONE));
                steps.push_back(key(
                    Key::Delete,
                    Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    },
                ));
            }
            for i in [3, 6, 9] {
                steps.push_back(Step::Click(Target::Clip(0, i)));
                steps.push_back(click("earlier"));
            }
            for i in 1..=4 {
                if i <= 2 {
                    steps.push_back(Step::Click(Target::Named(format!(
                        "media:microphone-{i}.wav"
                    ))));
                } else {
                    steps.push_back(key(
                        Key::ArrowDown,
                        Modifiers {
                            alt: true,
                            ..Modifiers::NONE
                        },
                    ));
                }
                steps.push_back(key(Key::Enter, Modifiers::COMMAND));
                prop(&mut steps, "Position (s)", &(5 + (i - 1) * 40).to_string());
                prop(&mut steps, "Fade in (s)", "1");
                prop(&mut steps, "Fade out (s)", "1");
                steps.push_back(key(Key::L, Modifiers::COMMAND));
                steps.push_back(Step::Wait("loudness"));
            }
            steps.push_back(Step::Click(Target::Fraction("track-gain-1".into(), 0.20)));
            steps.push_back(Step::Remember);
            steps.push_back(Step::Drag(
                Target::Named("media:microphone-1.wav".into()),
                Target::LaneAt(3, 170.),
            ));
            steps.push_back(Step::Assert("media-drop"));
            steps.push_back(key(Key::Z, Modifiers::COMMAND));
            steps.push_back(Step::Assert("restored"));
            steps.push_back(Step::Drag(Target::Clip(2, 0), Target::LaneAt(3, 7.)));
            steps.push_back(Step::Assert("audio-move"));
            steps.push_back(key(Key::Z, Modifiers::COMMAND));
            steps.push_back(Step::Assert("restored"));
            steps.push_back(Step::Drag(Target::Edge(0, 0, 0.), Target::Edge(0, 0, -2.)));
            steps.push_back(Step::Assert("trim-drag"));
            steps.push_back(key(Key::Z, Modifiers::COMMAND));
            steps.push_back(Step::Assert("restored"));
            steps.push_back(Step::Click(Target::Clip(0, 0)));
            steps.push_back(click("later"));
            steps.push_back(Step::Assert("video-move"));
            steps.push_back(key(Key::Z, Modifiers::COMMAND));
            steps.push_back(Step::Assert("restored"));
            steps.push_back(Step::Assert("edited"));
            steps.push_back(Step::Wait("preview"));
            steps.push_back(key(Key::Home, Modifiers::NONE));
            steps.push_back(key(Key::Space, Modifiers::NONE));
            steps.push_back(Step::Wait("played"));
            steps.push_back(key(Key::Space, Modifiers::NONE));
        } else {
            steps.push_back(Step::Wait("opened"));
            steps.push_back(Step::Assert("edited"));
            steps.push_back(Step::Click(Target::Clip(0, 0)));
            prop(&mut steps, "Duration (s)", "4.5");
        }
        if !stage.starts_with("crash-") {
            steps.push_back(key(Key::S, Modifiers::COMMAND));
            steps.push_back(Step::Wait("saved"));
            steps.push_back(key(Key::R, Modifiers::COMMAND));
            steps.push_back(Step::RenderPath);
            steps.push_back(click("render"));
            steps.push_back(Step::Wait("rendered"));
            steps.push_back(Step::CloseRender);
            steps.push_back(Step::Finish);
        }
        Self {
            root,
            steps,
            events: VecDeque::new(),
            started: Instant::now(),
            last_step: Instant::now(),
            index: 0,
            cooldown: 30,
            stage: stage.into(),
            done: false,
            report: vec![],
            baseline: None,
            last_report: Instant::now(),
        }
    }
    fn point(&self, target: &Target, app: &Editor, ctx: &egui::Context) -> Result<Pos2> {
        let named = |name: &str| {
            ctx.data(|d| d.get_temp::<Rect>(Id::new(("gui-probe", name))))
                .with_context(|| format!("Missing GUI widget: {name}"))
        };
        Ok(match target {
            Target::Named(name) => named(name)?.center(),
            Target::LeftEdge(lane, index, delta) | Target::Corner(lane, index, true, delta) => {
                let c = app
                    .p
                    .lane(*lane)
                    .get(*index)
                    .copied()
                    .context("Clip absent")?;
                let r = named(&format!("clip:{}", c.id))?;
                Pos2::new(
                    r.left()
                        + if matches!(target, Target::Corner(..)) {
                            5.
                        } else {
                            2.
                        }
                        + *delta as f32 * app.zoom,
                    if matches!(target, Target::Corner(..)) {
                        r.top() + 5.
                    } else {
                        r.center().y
                    },
                )
            }
            Target::Corner(lane, index, false, delta) => {
                let c = app
                    .p
                    .lane(*lane)
                    .get(*index)
                    .copied()
                    .context("Clip absent")?;
                let r = named(&format!("clip:{}", c.id))?;
                Pos2::new(r.right() - 5. + *delta as f32 * app.zoom, r.top() + 5.)
            }
            Target::Fraction(name, f) => {
                let r = named(name)?;
                Pos2::new(r.left() + r.width() * f, r.center().y)
            }
            Target::Clip(lane, index) => {
                let c = app
                    .p
                    .lane(*lane)
                    .get(*index)
                    .copied()
                    .context("Clip index is absent")?;
                let r = named(&format!("clip:{}", c.id))?;
                Pos2::new((r.left() + 10.).min(r.center().x), r.center().y)
            }
            Target::ClipAt(lane, t) => {
                let c = app
                    .p
                    .lane(*lane)
                    .into_iter()
                    .find(|c| ticks(*t) >= c.start && ticks(*t) < c.end())
                    .context("No clip at test time")?;
                let r = named(&format!("clip:{}", c.id))?;
                Pos2::new(
                    r.left() + (*t - seconds(c.start)) as f32 * app.zoom,
                    r.center().y,
                )
            }
            Target::Ruler(t) => {
                let r = named("ruler")?;
                Pos2::new(r.left() + *t as f32 * app.zoom, r.center().y)
            }
            Target::LaneAt(lane, t) => {
                let r = named(&format!("lane:{lane}"))?;
                Pos2::new(r.left() + *t as f32 * app.zoom, r.center().y)
            }
            Target::Edge(lane, index, delta) => {
                let c = app
                    .p
                    .lane(*lane)
                    .get(*index)
                    .copied()
                    .context("Clip absent")?;
                let r = named(&format!("clip:{}", c.id))?;
                Pos2::new(r.right() - 2. + *delta as f32 * app.zoom, r.center().y)
            }
        })
    }
    fn pointer(&mut self, p: Pos2) {
        self.events.push_back(egui::Event::PointerMoved(p));
        self.events.push_back(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        self.events.push_back(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
    }
    fn keyboard(&mut self, k: Key, m: Modifiers) {
        for pressed in [true, false] {
            self.events.push_back(egui::Event::Key {
                key: k,
                physical_key: Some(k),
                pressed,
                repeat: false,
                modifiers: m,
            });
        }
    }
    pub fn input(&mut self, app: &mut Editor, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.done {
            if app.capture_path.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        // Keep this raw-input regression independent of desktop focus/mouse
        // events while the user continues working in another application.
        raw.focused = true;
        raw.events
            .retain(|event| matches!(event, egui::Event::Screenshot { .. }));
        raw.modifiers = Modifiers::NONE;
        if self.last_report.elapsed() > Duration::from_secs(1) {
            self.last_report = Instant::now();
            let snapshot = app.player.as_ref().map(|p| p.snapshot());
            let live = serde_json::json!({"step":self.index,"next":format!("{:?}",self.steps.front()),"playhead":seconds(app.playhead),"revision":app.revision,"preview_revision":app.preview_revision,"loaded":snapshot.as_ref().map(|s|s.loaded),"paused":snapshot.as_ref().map(|s|s.paused),"position":snapshot.as_ref().map(|s|seconds(s.position)),"status":app.status});
            let _ = std::fs::write(
                self.root.join(format!("gui-live-{}.json", self.stage)),
                live.to_string(),
            );
        }
        if let Some(error) = &app.error {
            self.fail(
                app,
                ctx,
                format!("Application error at step {}: {error}", self.index),
            );
            return;
        }
        if self.started.elapsed() > Duration::from_secs(600) {
            self.fail(app, ctx, "GUI run exceeded 10 minutes".into());
            return;
        }
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return;
        }
        if let Some(event) = self.events.pop_front() {
            if self.stage == "1" && self.index < 14 {
                self.report.push(format!(
                    "INPUT {:?}; previous property={:?}",
                    event,
                    ctx.data(|d| d.get_temp::<String>(Id::new("duration-debug")))
                ));
            }
            if let egui::Event::Key { modifiers, .. } = &event {
                raw.modifiers = *modifiers;
            }
            raw.events.push(event);
            self.cooldown = 1;
            return;
        }
        let Some(step) = self.steps.front().cloned() else {
            return;
        };
        let result = (|| -> Result<bool> {
            match &step {
                Step::Drop => {
                    let media = self.root.join("acceptance-media");
                    for file in (1..=3)
                        .map(|i| format!("gameplay-{i}.mp4"))
                        .chain((1..=4).map(|i| format!("microphone-{i}.wav")))
                    {
                        raw.dropped_files.push(egui::DroppedFile {
                            path: Some(media.join(file)),
                            ..Default::default()
                        });
                    }
                }
                Step::Click(t) => self.pointer(self.point(t, app, ctx)?),
                Step::Remember => self.baseline = Some(app.p.clone()),
                Step::Drag(a, b) => {
                    let from = self.point(a, app, ctx)?;
                    let to = self.point(b, app, ctx)?;
                    self.events.push_back(egui::Event::PointerMoved(from));
                    self.events.push_back(egui::Event::PointerButton {
                        pos: from,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    });
                    for n in 1..=6 {
                        self.events.push_back(egui::Event::PointerMoved(
                            from + (to - from) * (n as f32 / 6.),
                        ));
                    }
                    self.events.push_back(egui::Event::PointerButton {
                        pos: to,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    });
                }
                Step::Text(s) => self.events.push_back(egui::Event::Text(s.clone())),
                Step::Key(k, m) => self.keyboard(*k, *m),
                Step::Wait(condition) => {
                    let ok = match *condition {
                        "imported" => app.p.media.len() == 7 && !app.importing,
                        "opened" => app.p.clips.len() == 30,
                        "autosave" => {
                            !app.autosaving
                                && app.dirty
                                && persistence::load(&app.recovery).ok().is_some_and(|p| {
                                    p.lane(1).first().is_some_and(|c| c.gain_db == -13.)
                                })
                        }
                        "loudness" => app.status.starts_with("Measured"),
                        "preview" => {
                            app.preview_revision == Some(app.revision)
                                && app.player.as_ref().is_some_and(|p| p.snapshot().loaded)
                        }
                        "played" => app.playhead > HZ * 2,
                        "saved" => !app.dirty && !app.saving,
                        "rendered" => app.rendered.is_some() && app.render_job.is_none(),
                        _ => false,
                    };
                    if !ok {
                        if self.last_step.elapsed() > Duration::from_secs(240) {
                            anyhow::bail!("Timed out waiting for {condition}");
                        }
                        return Ok(false);
                    }
                }
                Step::Assert(condition) => match *condition {
                    "linked-loudness" => anyhow::ensure!(
                        app.p.lane(0)[0].gain_db == 0. && app.p.lane(1)[0].gain_db.abs() > 0.1,
                        "Loudness shortcut on video must adjust its linked audio"
                    ),
                    "separate-linked" => {
                        app.p.validate()?;
                        anyhow::ensure!(
                            app.p.lane(0).len() == 1
                                && app.p.lane(1).len() == 1
                                && app.p.lane(0)[0].link.is_some(),
                            "Video and source audio must have linked separate rows"
                        );
                    }
                    "one-click-split" => {
                        app.p.validate()?;
                        anyhow::ensure!(
                            app.p.lane(0).len() == 2
                                && app.p.lane(1).len() == 2
                                && app.p.lane(0)[0].duration == HZ * 2,
                            "One click plus S must split both clips at 2s"
                        );
                    }
                    "trim-four" => {
                        app.p.validate()?;
                        anyhow::ensure!(
                            app.p.lane(0)[0].start == 0
                                && (app.p.lane(0)[0].duration - HZ * 4).abs() < HZ / 30,
                            "Trim must keep start fixed and linked sound aligned"
                        );
                    }
                    "extend-five" => {
                        app.p.validate()?;
                        anyhow::ensure!(
                            app.p.lane(0)[0].start == 0
                                && (app.p.lane(0)[0].duration - HZ * 5).abs() < HZ / 30,
                            "Extending right edge must not move the clip"
                        );
                    }
                    "left-trim" => {
                        app.p.validate()?;
                        let c = app.p.lane(0)[0];
                        anyhow::ensure!(
                            (c.start - HZ).abs() < HZ / 30
                                && (c.end() - HZ * 5).abs() < HZ / 30
                                && c.source_in == c.start,
                            "Left edge must follow pointer while end stays fixed"
                        );
                    }
                    "video-fade" => anyhow::ensure!(
                        app.p.lane(0)[0].fade_in > HZ / 3,
                        "Top corner video fade handle did not apply"
                    ),
                    "audio-fade" => anyhow::ensure!(
                        app.p.lane(1)[0].fade_out > HZ / 3,
                        "Top corner audio fade handle did not apply"
                    ),
                    "unlink-move" => {
                        app.p.validate()?;
                        anyhow::ensure!(
                            app.p.lane(1).is_empty()
                                && app.p.lane(2).len() == 1
                                && app.p.lane(0)[0].start == HZ
                                && app.p.lane(0)[0].link.is_none(),
                            "Unlinked sound must move independently"
                        );
                    }

                    "recovered" => {
                        anyhow::ensure!(
                            app.path.is_none() && app.dirty && app.recovered_unsaved,
                            "Recovery must remain unsaved and separate from the manual project"
                        );
                        anyhow::ensure!(
                            app.p.lane(1)[0].gain_db == -13.,
                            "Recovery lost the last autosaved edit"
                        );
                    }
                    "media-drop" => anyhow::ensure!(
                        app.p.clips.len() == 31 && app.p.lane(3).len() == 1,
                        "Media drag did not insert audio"
                    ),
                    "audio-move" => anyhow::ensure!(
                        app.p.clips.len() == 30 && app.p.lane(3).len() == 1,
                        "Audio drag did not change lanes"
                    ),
                    "trim-drag" => anyhow::ensure!(
                        app.p.duration() == HZ * 230
                            && app.p.lane(0)[0].duration
                                == self.baseline.as_ref().unwrap().lane(0)[0].duration - HZ * 2,
                        "Edge drag did not trim exactly two seconds: {}",
                        seconds(app.p.duration())
                    ),
                    "video-move" => anyhow::ensure!(
                        self.baseline.as_ref() != Some(&app.p),
                        "Video drag did not reorder"
                    ),
                    "restored" => anyhow::ensure!(
                        self.baseline.as_ref() == Some(&app.p),
                        "Undo failed to restore drag operation"
                    ),
                    "assembled" => anyhow::ensure!(
                        app.p.clips.len() == 6 && app.p.duration() == HZ * 240,
                        "GUI assembly or duration edits failed: clips {}, duration {}",
                        app.p.clips.len(),
                        seconds(app.p.duration())
                    ),
                    "edited" => {
                        anyhow::ensure!(
                            app.p.duration() == HZ * 230,
                            "Ripple deletion did not close ten one-second gaps: duration {}",
                            seconds(app.p.duration())
                        );
                        let sources: std::collections::HashSet<_> =
                            app.p.lane(2).iter().map(|c| c.media.clone()).collect();
                        anyhow::ensure!(
                            sources.len() == 4,
                            "Four distinct microphones must be used"
                        );
                        anyhow::ensure!(
                            app.p.lane(0).len() == 13
                                && app.p.lane(1).len() == 13
                                && app.p.lane(2).len() == 4,
                            "GUI cuts/audio placement failed"
                        );
                        anyhow::ensure!(app.p.tracks[1].gain_db < -3., "Gameplay was not lowered");
                        for c in app.p.lane(2) {
                            anyhow::ensure!(
                                c.gain_db.abs() > 0.1 && c.fade_in == HZ && c.fade_out == HZ,
                                "Voice gain/fades were not applied"
                            );
                        }
                    }
                    _ => {}
                },
                Step::RenderPath => {
                    self.pointer(self.point(&Target::Named("render-path".into()), app, ctx)?);
                    self.keyboard(Key::A, Modifiers::COMMAND);
                    self.events.push_back(egui::Event::Text(
                        self.root
                            .join(format!("gui-export-{}.mp4", self.stage))
                            .to_string_lossy()
                            .into(),
                    ));
                    self.keyboard(Key::Enter, Modifiers::NONE);
                }
                Step::CloseRender => {
                    app.render_dialog = false;
                }
                Step::Finish => {
                    if self.stage == "qol" {
                        anyhow::ensure!(
                            app.p.lane(0)[0].link.is_some(),
                            "Undo must restore linkage"
                        );
                    }
                    let path = self.root.join(format!("gui-report-{}.json", self.stage));
                    let report = serde_json::json!({"passed":true,"stage":self.stage,"steps":self.index,"elapsed_seconds":self.started.elapsed().as_secs_f64(),"encoder_used":app.encoder_used,"clips":app.p.clips.len(),"duration_seconds":seconds(app.p.duration()),"verified":"Native eframe window; egui raw pointer/keyboard/drop events; live media adapters; real save and export","events":self.report});
                    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                    self.done = true;
                    app.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Step::Crash => {
                    let original =
                        persistence::load(app.path.as_ref().context("No original project")?)?;
                    anyhow::ensure!(
                        original.lane(1)[0].gain_db != -13.,
                        "Autosave overwrote manual save"
                    );
                    let report = serde_json::json!({"passed":true,"steps":self.index,"recovery":app.recovery,"saved_project":app.path,"manual_gain":original.lane(1)[0].gain_db,"recovery_gain": -13,"termination":"intentional process exit 86 after autosave"});
                    std::fs::write(
                        self.root.join("gui-report-crash-write.json"),
                        serde_json::to_vec_pretty(&report)?,
                    )?;
                    std::process::exit(86);
                }
            }
            Ok(true)
        })();
        match result {
            Ok(true) => {
                self.report.push(format!(
                    "{}: {:?}; clips={} selected={:?} media={:?}; duration={:?}; property={:?}",
                    self.index,
                    step,
                    app.p.clips.len(),
                    app.selected,
                    app.media_selected,
                    app.p
                        .lane(0)
                        .iter()
                        .map(|c| seconds(c.duration))
                        .collect::<Vec<_>>(),
                    ctx.data(|d| d.get_temp::<String>(Id::new("duration-debug")))
                ));
                self.steps.pop_front();
                self.index += 1;
                self.last_step = Instant::now();
                self.cooldown = 3;
            }
            Ok(false) => {}
            Err(e) => self.fail(app, ctx, format!("Step {} ({step:?}): {e:#}", self.index)),
        }
    }
    fn fail(&mut self, app: &mut Editor, _ctx: &egui::Context, error: String) {
        let _=std::fs::write(self.root.join(format!("gui-report-{}.json",self.stage)),serde_json::json!({"passed":false,"error":error,"steps":self.index,"events":self.report}).to_string());
        self.done = true;
        app.allow_close = true;
        app.capture_path = Some(self.root.join(format!("gui-failure-{}.png", self.stage)));
        app.capture_requested = false;
        app.last_edit = Instant::now() - Duration::from_secs(3);
    }
}
