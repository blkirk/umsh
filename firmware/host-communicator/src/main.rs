//! A communicator on the host. The screen is a window at the T-LoRa
//! Pager's 480x222; the keyboard is the Pager's keyboard.
//!
//! Interactive (needs the `window` feature):
//!
//!     host-communicator
//!
//! Up and Down arrows or the mouse wheel turn the wheel; the Right arrow or
//! a click presses it. Return, Backspace, and the letters are themselves.
//!
//! Headless, for scripted runs and comparisons with the mockup:
//!
//!     host-communicator --script steps.txt --out shots/
//!
//! Each script line is one step: `turn N` (negative is up), `press`,
//! `enter`, `backspace`, `type TEXT`, or `shot NAME`, which writes
//! `NAME.png` into the output directory. `#` starts a comment.
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
use buoyant::render_target::{EmbeddedGraphicsRenderTarget, RenderTarget as _};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::{OutputSettings, SimulatorDisplay};
use umsh_ux_communicator::input::{self, Input};
use umsh_ux_communicator::{fixture, theme, ui};

const SIZE: Size = Size::new(theme::WIDTH, theme::HEIGHT);

/// Where the fixture's chats keep their messages, for the life of the run.
fn store() -> &'static fixture::Store {
    Box::leak(Box::new(fixture::Store::new(&std::alloc::System)))
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
    let mut app = App::new(fixture::state(store()), SIZE.into(), ui::root_view);
    ui::bind(&mut app);
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
            "shot" => {
                let mut target =
                    EmbeddedGraphicsRenderTarget::new_hinted(&mut display, theme::BACKGROUND);
                target.clear(theme::BACKGROUND);
                // Headless runs are not animated: draw where things settle.
                app.set_time(std::time::Duration::from_secs(3600));
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
    let mut app = App::new(fixture::state(store()), SIZE.into(), ui::root_view);
    ui::bind(&mut app);
    loop {
        app.set_time(start.elapsed());
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
            }
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
