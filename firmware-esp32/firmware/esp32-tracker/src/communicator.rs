//! The communicator interface on the Pager's panel, in place of the
//! tracker's menus. For now it shows the fixture's chats and reports what
//! each frame costs: layout, painting, the panel transfer, and
//! the least stack left free.
use super::*;
use board::display::Display;
use umsh_ux_communicator::buoyant::app::App;
use umsh_ux_communicator::buoyant::render_target::{
    EmbeddedGraphicsRenderTarget, RenderTarget as _,
};
use umsh_ux_communicator::buoyant::view::View;
use umsh_ux_communicator::input::{self, Input};
use umsh_ux_communicator::model::State;
use umsh_ux_communicator::{fixture, theme, ui};
use umsh_ux_display_tracker::menu::UiInput;

/// How often an animation advances.
const FRAME: Duration = Duration::from_millis(16);
/// How often the costs are reported.
const REPORT: Duration = Duration::from_secs(1);

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
    // The interface's state and both of its render trees are plain data
    // that only this task touches, and they are the largest thing it
    // holds: kept in the task, they would come out of the main stack.
    let app = external::communicator(|| {
        App::new(
            fixture::state(),
            embedded_graphics::prelude::Size::new(theme::WIDTH, theme::HEIGHT).into(),
            ui::root_view,
        )
    });
    debug_log(format_args!(
        "communicator: app={} bytes",
        core::mem::size_of_val(app)
    ));
    let mut costs = Costs::default();
    let mut animating = draw(&mut display, app, start, &mut costs).await;
    let _ = display.set_display_on(true).await;
    pager::input::shown(epoch);

    loop {
        let wait = if animating { FRAME } else { REPORT };
        if let Either::First(input) = select(UI_INPUT_CH.receive(), Timer::after(wait)).await {
            input::apply(
                app,
                match input {
                    UiInput::Forward => Input::Turn(1),
                    UiInput::Backward => Input::Turn(-1),
                    UiInput::Select => Input::Press,
                    UiInput::Back => Input::Backspace,
                },
            );
        }
        if animating || app.should_redraw() {
            animating = draw(&mut display, app, start, &mut costs).await;
        }
        costs.report(animating);
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
        // ms, and the least stack left free so far.
        #[cfg(feature = "debug-log")]
        debug_log(format_args!(
            "ui f={} a={} l={} p={} s={} free={}",
            self.frames,
            u8::from(animating),
            self.layout.as_millis(),
            self.paint.as_millis(),
            self.flush.as_millis(),
            memory_probe::stack().margin,
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
