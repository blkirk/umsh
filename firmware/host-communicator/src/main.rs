//! A communicator on the host. The screen is a window at the T-LoRa
//! Pager's 480x222; the keyboard is the Pager's keyboard.
//!
//! Interactive (needs the `window` feature):
//!
//!     host-communicator
//!
//! Up and Down arrows or the mouse wheel turn the wheel; the Right arrow or
//! a click presses it. Return, Backspace, and the letters are themselves.
//! Tab brings the fixture's next message in, as if it arrived. An
//! identity asked for arrives two seconds later.
//!
//! Headless, for scripted runs and comparisons with the mockup:
//!
//!     host-communicator --script steps.txt --out shots/
//!
//! Each script line is one step: `turn N` (negative is up), `press`,
//! `enter`, `backspace`, `type TEXT`, `airtime SECONDS` (spent airtime
//! that returns in that long; 0 for none), `arrive` (the fixture's next
//! message arrives), `identify` (the identity asked for arrives), `wait
//! SECONDS` (the board's clock moves on), `dark` and `lit` (the panel goes
//! dark and lights again), `battery PERCENT [charging]`, `battery-empty` (with
//! `off` for the blink's dark half), `first-run`, or `shot NAME`, which writes `NAME.png` into the output directory. `#`
//! starts a comment.
//!
//! Timing, and something for a profiler to watch:
//!
//!     host-communicator --bench N
//!
//! Turns the wheel N times, down the list and back, and reports the mean
//! layout and paint time of a frame.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use umsh_ux_communicator::buoyant;

use buoyant::app::App;
use buoyant::focus::DefaultFocus;
use buoyant::render::AnimatedJoin;
use buoyant::render_target::{EmbeddedGraphicsRenderTarget, RenderTarget as _};
use buoyant::view::ViewLayout;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::{OutputSettings, SimulatorDisplay};
use umsh_ux_communicator::input::{self, Input};
use umsh_ux_communicator::model::State;
use umsh_ux_communicator::{fixture, theme, ui};

const SIZE: Size = Size::new(theme::WIDTH, theme::HEIGHT);

/// Where the fixture's chats keep their messages, for the life of the run.
fn store() -> &'static fixture::Store {
    Box::leak(Box::new(fixture::Store::new(&std::alloc::System)))
}

/// Takes in the fixture's next message to arrive, at `now` in milliseconds,
/// as a board would.
fn arrive<V, F>(app: &mut App<V, State, F>, store: &fixture::Store, now: u64)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    if let Some((chat, message)) = store.arrive() {
        app.state_mut().receive(chat, message, now);
        app.finalize_view();
    }
}

/// How long an identity takes to arrive once asked for, in the window.
#[cfg(feature = "window")]
const IDENTITY_DELAY_MS: u64 = 2000;

/// Takes in the identity asked for, as a board would, if one was.
fn identify<V, F>(app: &mut App<V, State, F>, store: &fixture::Store)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    if store.answer_identity() {
        app.state_mut().identities_arrived();
        app.finalize_view();
    }
}

/// Lets go of whatever on screen has had its time by `now`.
fn expire<V, F>(app: &mut App<V, State, F>, now: u64)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    if app.state().next_deadline().is_some_and(|at| at <= now) {
        app.state_mut().expire(now);
        app.finalize_view();
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => window(),
        [script_flag, script, out_flag, out]
            if script_flag == "--script" && out_flag == "--out" =>
        {
            match headless(Path::new(script), &PathBuf::from(out)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("host-communicator: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        [bench_flag, count] if bench_flag == "--bench" => match count.parse() {
            Ok(count) => {
                bench(count);
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!("host-communicator: --bench needs a count");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("usage: host-communicator [--script FILE --out DIR | --bench N]");
            ExitCode::from(2)
        }
    }
}

fn bench(count: u32) {
    use std::time::{Duration, Instant};

    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SIZE);
    let mut app = App::new(fixture::state(store()), SIZE.into(), ui::root_view);
    ui::bind(&mut app);
    let (mut layout, mut paint) = (Duration::ZERO, Duration::ZERO);
    for turn in 0..count {
        // Four steps down, four back, so the focus keeps moving.
        let step = if turn / 4 % 2 == 0 { 1 } else { -1 };
        input::apply(&mut app, Input::Turn(step));
        let began = Instant::now();
        app.finalize_view();
        let laid_out = Instant::now();
        let mut target = EmbeddedGraphicsRenderTarget::new_hinted(&mut display, theme::BACKGROUND);
        app.render_animated(&mut target, &theme::TEXT);
        paint += laid_out.elapsed();
        layout += laid_out - began;
    }
    let count = count.max(1);
    println!(
        "{count} frames: layout {:?}, paint {:?} per frame",
        layout / count,
        paint / count
    );
}

fn headless(script: &Path, out: &Path) -> Result<(), String> {
    let steps =
        std::fs::read_to_string(script).map_err(|e| format!("{}: {e}", script.display()))?;
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SIZE);
    let store = store();
    let mut app = App::new(fixture::state(store), SIZE.into(), ui::root_view);
    ui::bind(&mut app);
    // The board's clock, which only `wait` moves.
    let mut clock = 0;
    for (number, line) in steps.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let input = match word {
            "turn" => {
                Some(Input::Turn(rest.parse().map_err(|_| {
                    format!("line {}: turn needs a count", number + 1)
                })?))
            }
            "press" => Some(Input::Press),
            "enter" => Some(Input::Enter),
            "backspace" => Some(Input::Backspace),
            "type" => {
                for c in rest.chars() {
                    input::apply(&mut app, Input::Character(c));
                }
                None
            }
            "dark" | "lit" => {
                app.state_mut().dark = word == "dark";
                app.finalize_view();
                None
            }
            "arrive" => {
                arrive(&mut app, store, clock);
                None
            }
            "identify" => {
                identify(&mut app, store);
                None
            }
            "wait" => {
                let seconds: u64 = rest
                    .parse()
                    .map_err(|_| format!("line {}: wait needs seconds", number + 1))?;
                clock += seconds * 1000;
                expire(&mut app, clock);
                None
            }
            "battery" => {
                // `battery PERCENT`, with `charging` after it while plugged in.
                let mut words = rest.split_whitespace();
                let percent: u8 = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| format!("line {}: battery needs a percent", number + 1))?;
                let charging = words.next() == Some("charging");
                app.state_mut().battery(Some(percent), charging, clock);
                app.finalize_view();
                None
            }
            "first-run" => {
                app.state_mut().start_setup();
                app.finalize_view();
                None
            }
            "battery-empty" => {
                // `battery-empty off` draws the blink's dark half.
                app.state_mut().battery_empty(rest != "off");
                app.finalize_view();
                None
            }
            "airtime" => {
                let wait: u16 = rest
                    .parse()
                    .map_err(|_| format!("line {}: airtime needs seconds", number + 1))?;
                app.state_mut().airtime_wait = (wait > 0).then_some(wait);
                app.finalize_view();
                None
            }
            "shot" => {
                let mut target =
                    EmbeddedGraphicsRenderTarget::new_hinted(&mut display, theme::BACKGROUND);
                target.clear(theme::BACKGROUND);
                // Headless runs are not animated: draw where things settle,
                // well after what the last steps started.
                app.advance_time(std::time::Duration::from_secs(60));
                app.render_animated(&mut target, &theme::TEXT);
                let path = out.join(format!("{rest}.png"));
                display
                    .to_rgb_output_image(&OutputSettings::default())
                    .save_png(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                None
            }
            other => return Err(format!("line {}: unknown step {other:?}", number + 1)),
        };
        if let Some(input) = input {
            input::apply(&mut app, input);
        }
    }
    Ok(())
}

#[cfg(feature = "window")]
fn window() -> ExitCode {
    use embedded_graphics_simulator::sdl2::{Keycode, Mod, MouseButton};
    use embedded_graphics_simulator::{SimulatorEvent, Window};

    let mut window = Window::new(
        "UMSH communicator",
        &OutputSettings {
            scale: 2,
            ..OutputSettings::default()
        },
    );
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SIZE);
    let mut target = EmbeddedGraphicsRenderTarget::new_hinted(&mut display, theme::BACKGROUND);
    window.update(target.display());
    let start = std::time::Instant::now();
    let store = store();
    let mut app = App::new(fixture::state(store), SIZE.into(), ui::root_view);
    ui::bind(&mut app);
    // When the identity asked for arrives.
    let mut identity_at: Option<u64> = None;
    loop {
        app.set_time(start.elapsed());
        let now = start.elapsed().as_millis() as u64;
        expire(&mut app, now);
        for event in window.events() {
            let input = match event {
                SimulatorEvent::Quit => return ExitCode::SUCCESS,
                SimulatorEvent::KeyDown {
                    keycode, keymod, ..
                } => match keycode {
                    Keycode::Up => Some(Input::Turn(-1)),
                    Keycode::Down => Some(Input::Turn(1)),
                    Keycode::Right => Some(Input::Press),
                    Keycode::Return => Some(Input::Enter),
                    Keycode::Backspace => Some(Input::Backspace),
                    Keycode::Space => Some(Input::Character(' ')),
                    Keycode::Tab => {
                        arrive(&mut app, store, now);
                        None
                    }
                    other => {
                        // Letters and digits, by their key names.
                        let shifted = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
                        let name = other.name();
                        let mut chars = name.chars();
                        match (chars.next(), chars.next()) {
                            (Some(c), None) if shifted => Some(Input::Character(c)),
                            (Some(c), None) => Some(Input::Character(c.to_ascii_lowercase())),
                            _ => None,
                        }
                    }
                },
                SimulatorEvent::MouseWheel { scroll_delta, .. } => {
                    Some(Input::Turn(if scroll_delta.y > 0 { -1 } else { 1 }))
                }
                SimulatorEvent::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    ..
                } => Some(Input::Press),
                _ => None,
            };
            if let Some(input) = input {
                input::apply(&mut app, input);
                // An identity asked for arrives a moment later.
                if store.identity_asked() {
                    identity_at = Some(now + IDENTITY_DELAY_MS);
                }
            }
        }
        if identity_at.is_some_and(|at| at <= now) {
            identity_at = None;
            identify(&mut app, store);
        }
        if app.should_redraw() || target.clear_animation_status() {
            target.clear(theme::BACKGROUND);
            app.render_animated(&mut target, &theme::TEXT);
            window.update(target.display());
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

#[cfg(not(feature = "window"))]
fn window() -> ExitCode {
    eprintln!("host-communicator: built without the window feature; use --script");
    ExitCode::from(2)
}
