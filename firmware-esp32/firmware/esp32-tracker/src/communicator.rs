//! The communicator interface on the Pager's panel, in place of the
//! tracker's menus. For now it shows the fixture's chats with the board's
//! own battery, taking in what the fixture says as it would a node's
//! events, and reports what each frame costs: layout, painting, the panel
//! transfer, and the least stack left free.
use super::*;
use board::display::{Brightness, Display};
use umsh_ux_communicator::buoyant::app::App;
use umsh_ux_communicator::buoyant::render_target::{
    EmbeddedGraphicsRenderTarget, RenderTarget as _,
};
use umsh_ux_communicator::buoyant::view::View;
use umsh_ux_communicator::input::{self, Input};
use umsh_ux_communicator::keyboard::Modifier;
use umsh_ux_communicator::model::{State, brightness_level};
use umsh_ux_communicator::{fixture, theme, ui};
use umsh_ux_display_tracker::attention::{Attention, AttentionConfig, DisplayKind, Transition};
use umsh_ux_display_tracker::menu::{KeyModifier, UiInput};

/// How often an animation advances.
const FRAME: Duration = Duration::from_millis(16);
/// How often the costs are reported.
const REPORT: Duration = Duration::from_secs(1);
/// When the fixture's next message arrives, so that arrivals can be tried
/// before a radio feeds the interface: soon after an input, while the panel
/// is lit, and otherwise as the fixture paces them, once it has gone dark.
const ARRIVAL_AFTER_INPUT: Duration = Duration::from_secs(8);
/// How long Shutting down shows before the panel goes dark.
const SHUTTING_DOWN: Duration = Duration::from_millis(1500);
/// The empty battery's blink before the power path opens: how many times,
/// and how long it is lit and dark each time. Inside the two seconds
/// [`ui::end_farewell`](crate::ui::end_farewell) waits.
const EMPTY_BLINKS: u8 = 3;
const EMPTY_LIT: Duration = Duration::from_millis(300);
const EMPTY_DARK: Duration = Duration::from_millis(200);

#[embassy_executor::task]
pub(crate) async fn task(
    mut display: Display,
    mut splash: umsh_ux_display_tracker::boot::BootSplash,
) {
    let epoch = pager::input::interactive().await;
    if let Some(deadline) = splash.next_deadline() {
        Timer::at(Instant::from_millis(deadline)).await;
    }
    splash.dismiss();
    BOOT_SPLASH_ACTIVE.store(false, Ordering::Release);

    let start = Instant::now();
    let store = external::communicator_fixture();
    // The fixture's clock runs with the board's from here.
    store.advance(Instant::now().as_millis());
    // The interface's state and both of its render trees are plain data
    // that only this task touches, and they are the largest thing it
    // holds: kept in the task, they would come out of the main stack.
    let app = external::communicator(|slot| {
        let app = App::new_in(
            slot,
            fixture::state(store),
            embedded_graphics::prelude::Size::new(theme::WIDTH, theme::HEIGHT).into(),
            ui::root_view,
        );
        ui::bind(app);
        app
    });
    debug_log(format_args!(
        "communicator: app={} bytes",
        core::mem::size_of_val(app)
    ));
    app.state_mut()
        .battery(battery_percent(), battery_charging(), 0);
    let mut costs = Costs::default();
    let node = app.state().node;
    // The panel's brightness level and the keyboard light, as last set.
    let mut lights = (brightness_level(node.brightness()), node.keyboard().light);
    let _ = display.set_brightness(Brightness::level(lights.0)).await;
    display.set_keyboard_light(lights.1);
    let mut animating = draw(&mut display, app, start, &mut costs).await;
    let _ = display.set_display_on(true).await;
    pager::input::shown(epoch);
    // The panel dims, then goes dark, after a while without input, as the
    // tracker's does. An input against a fading or dark panel only wakes
    // it: the input layer reads `SCREEN_FADED` and consumes the gesture.
    let mut attention = Attention::new(
        DisplayKind::Emissive,
        AttentionConfig::EMISSIVE,
        Instant::now().as_millis(),
    );

    // When the fixture's clock next turns to a new minute, which ages are
    // counted to.
    let mut minute = Instant::now() + Duration::from_millis(store.millis_to_next_minute());

    loop {
        SCREEN_FADED.store(attention.is_faded(), Ordering::Release);
        let lit = attention.accepts_redraw();
        let tick = Instant::now() + if animating && lit { FRAME } else { REPORT };
        let wake_at = [
            attention.next_deadline(),
            app.state().next_deadline(),
            store.next_deadline(),
            Some(minute.as_millis()),
        ]
        .into_iter()
        .flatten()
        .fold(tick, |at, deadline| at.min(Instant::from_millis(deadline)));
        let now = || Instant::now().as_millis();
        let woken = select4(
            UI_INPUT_CH.receive(),
            UI_WAKE.wait(),
            Timer::at(wake_at),
            select(BATTERY_UI_CHANGED.wait(), DISPLAY_SHUTDOWN.wait()),
        )
        .await;
        // Whatever this changes animates from now, not from the last frame.
        app.set_time((Instant::now() - start).into());
        let mut transition = match woken {
            Either4::First(first) => {
                // An input leaves the interface laid out and settled, so its
                // layout is counted here rather than in the frame. Everything
                // queued is taken before drawing, so typing outpacing the
                // frames only skips frames.
                let began = Instant::now();
                let mut next = Some(first);
                while let Some(ui) = next {
                    input::apply(app, translate(ui), now());
                    next = UI_INPUT_CH.try_receive().ok();
                }
                costs.layout = costs.layout.max(Instant::now() - began);
                store.pace_arrivals((Instant::now() + ARRIVAL_AFTER_INPUT).as_millis());
                // Shut down, confirmed.
                if app.state().ending.is_some() {
                    SHUTDOWN_REQUEST.signal(ShutdownCause::Asked);
                }
                attention.wake(now())
            }
            Either4::Second(()) => attention.wake(now()),
            Either4::Third(()) => attention.poll(now()),
            // A reading redraws the strip but never lights the panel.
            Either4::Fourth(Either::First(())) => {
                app.state_mut()
                    .battery(battery_percent(), battery_charging(), now());
                None
            }
            Either4::Fourth(Either::Second(cause)) => farewell(&mut display, app, cause).await,
        };
        if app.state().next_deadline().is_some_and(|at| at <= now()) {
            app.state_mut().expire(now());
        }
        store.advance(now());
        if minute <= Instant::now() {
            app.state_mut().set_wall_time(Some(store.time()));
            minute = Instant::now() + Duration::from_millis(store.millis_to_next_minute());
        }
        // Taken in while the panel is still dark, so that a message
        // arriving then is listed for the screen it wakes to.
        if input::events(app, now()) {
            pager::alert::tick();
            if let Some(woke) = attention.wake(now()) {
                transition = Some(woke);
            }
        }
        // A key, or the node, may have changed the lights.
        let set = (brightness_level(node.brightness()), node.keyboard().light);
        if set != lights {
            lights = set;
            let _ = display.set_brightness(Brightness::level(lights.0)).await;
            display.set_keyboard_light(lights.1);
        }
        match transition {
            Some(Transition::Woke) => {
                if app.state().dark {
                    app.state_mut().dark = false;
                }
                // A fresh frame goes in before the panel lights, so a stale
                // one is never seen.
                let epoch = pager::input::interactive().await;
                animating = draw(&mut display, app, start, &mut costs).await;
                let _ = display.set_brightness(Brightness::level(lights.0)).await;
                if display.set_display_on(true).await.is_ok() {
                    pager::input::shown(epoch);
                }
                continue;
            }
            Some(Transition::Dimming) => {
                let dimmed = Brightness::level(lights.0).dimmed(attention.brightness_permille());
                let _ = display.set_brightness(dimmed).await;
            }
            Some(Transition::Lapsed) => {
                let before = pager::input::activity();
                if display.set_display_on(false).await.is_ok() {
                    pager::input::dark(before);
                }
                app.state_mut().dark = true;
            }
            None => {}
        }
        if attention.accepts_redraw() && (animating || app.should_redraw()) {
            animating = draw(&mut display, app, start, &mut costs).await;
        }
        costs.report(animating);
    }
}

/// The charge the board last read, in percent.
fn battery_percent() -> Option<u8> {
    Some(BATTERY_LEVEL.load(Ordering::Acquire)).filter(|&level| level != 0xFF)
}

/// Whether external power is charging the battery, or has charged it.
fn battery_charging() -> bool {
    matches!(BATTERY_CHARGE.load(Ordering::Acquire), 2 | 3)
}

/// The board is turning off, and the panel says so: Shutting down, or with
/// the battery empty its icon blinking a few times. Then the panel goes
/// dark, and nothing more is drawn.
async fn farewell<V, F>(
    display: &mut Display,
    app: &mut App<V, State, F>,
    cause: ShutdownCause,
) -> !
where
    V: View<embedded_graphics::pixelcolor::Rgb565, State>,
    F: Fn(&State) -> V,
{
    let start = Instant::now();
    let mut costs = Costs::default();
    // Each frame shown lit at full brightness, whatever the panel was doing.
    match cause {
        ShutdownCause::Asked => {
            app.state_mut().shutting_down();
            draw(display, app, start, &mut costs).await;
            let _ = display.set_brightness(Brightness::NORMAL).await;
            let _ = display.set_display_on(true).await;
            Timer::after(SHUTTING_DOWN).await;
        }
        ShutdownCause::BatteryEmpty => {
            for _ in 0..EMPTY_BLINKS {
                for (lit, hold) in [(true, EMPTY_LIT), (false, EMPTY_DARK)] {
                    app.state_mut().battery_empty(lit);
                    draw(display, app, start, &mut costs).await;
                    if lit {
                        let _ = display.set_brightness(Brightness::NORMAL).await;
                        let _ = display.set_display_on(true).await;
                    }
                    Timer::after(hold).await;
                }
            }
        }
    }
    let _ = display.set_display_on(false).await;
    DISPLAY_SHUTDOWN_DONE.signal(());
    core::future::pending().await
}

fn translate(input: UiInput) -> Input {
    match input {
        UiInput::Forward => Input::Turn(1),
        UiInput::Backward => Input::Turn(-1),
        UiInput::Select => Input::Press,
        UiInput::Back => Input::Backspace,
        UiInput::Enter => Input::Enter,
        UiInput::Key { plain, alt } => Input::Key { plain, alt },
        UiInput::Space => Input::Space,
        UiInput::Modifier { modifier, down } => Input::Modifier {
            modifier: match modifier {
                KeyModifier::Shift => Modifier::Shift,
                KeyModifier::Alt => Modifier::Alt,
            },
            down,
        },
    }
}

/// The frames since the last report, and the worst of each stage. Reported
/// at most once per [`REPORT`], on one short line: the Pager's debug stream
/// holds one 64-byte line at a time.
#[derive(Default)]
struct Costs {
    frames: u32,
    layout: Duration,
    paint: Duration,
    flush: Duration,
    since: Option<Instant>,
}

impl Costs {
    fn report(&mut self, animating: bool) {
        let since = *self.since.get_or_insert_with(Instant::now);
        if self.frames == 0 || Instant::now() - since < REPORT {
            return;
        }
        // Frames, animating, the worst layout, paint, and flush in
        // ms, the least stack left free so far, and the CPU clock.
        #[cfg(feature = "debug-log")]
        debug_log(format_args!(
            "ui f={} a={} l={} p={} s={} free={} mhz={}",
            self.frames,
            u8::from(animating),
            self.layout.as_millis(),
            self.paint.as_millis(),
            self.flush.as_millis(),
            memory_probe::stack().margin,
            esp_hal::clock::cpu_clock().as_mhz(),
        ));
        #[cfg(not(feature = "debug-log"))]
        let _ = animating;
        *self = Self {
            since: Some(Instant::now()),
            ..Self::default()
        };
    }
}

/// Lays out, draws, and sends one frame. Returns whether an animation is
/// still running.
async fn draw<V, F>(
    display: &mut Display,
    app: &mut App<V, State, F>,
    start: Instant,
    costs: &mut Costs,
) -> bool
where
    V: View<embedded_graphics::pixelcolor::Rgb565, State>,
    F: Fn(&State) -> V,
{
    let began = Instant::now();
    app.set_time((began - start).into());
    app.finalize_view();
    let laid_out = Instant::now();
    let animating = {
        let mut target =
            EmbeddedGraphicsRenderTarget::new_hinted(display.color(), theme::BACKGROUND);
        // A new target reports an animation until told otherwise, so that
        // a first frame always draws; this one is made fresh every frame.
        target.clear_animation_status();
        // No clear: the root view paints the whole screen.
        app.render_animated(&mut target, &theme::TEXT);
        target.clear_animation_status()
    };
    let painted = Instant::now();
    // A failed transfer leaves the panel's history invalid, so the next
    // frame resends everything; there is nothing more to do here.
    let _ = display.flush().await;
    let sent = Instant::now();
    costs.frames += 1;
    costs.layout = costs.layout.max(laid_out - began);
    costs.paint = costs.paint.max(painted - laid_out);
    costs.flush = costs.flush.max(sent - painted);
    animating
}
