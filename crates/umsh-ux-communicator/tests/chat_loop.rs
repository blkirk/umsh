//! The chat loop driven the way a board drives it: inputs through
//! `input::apply`, with no frame drawn between them.

use buoyant::app::App;
use buoyant::focus::{DefaultFocus, FocusKey};
use buoyant::render::AnimatedJoin;
use buoyant::view::ViewLayout;
use embedded_graphics::prelude::Size;
use umsh_ux_communicator::buoyant;
use umsh_ux_communicator::input::{self, Input};
use umsh_ux_communicator::model::{Delivery, Direction, Reaction, Section, State};
use umsh_ux_communicator::nav::{Screen, keys};
use umsh_ux_communicator::{fixture, theme, ui};

/// The fixture's chats, in list order.
const RIDGE: u32 = 1;
const PAT: u32 = 3;
const RIDGE_DRAFT: &str = "On my way, save me some hot coffee";

macro_rules! app {
    () => {{
        let mut app = App::new(
            fixture::state(Box::leak(Box::new(fixture::Store::new(
                &std::alloc::System,
            )))),
            Size::new(theme::WIDTH, theme::HEIGHT).into(),
            ui::root_view,
        );
        ui::bind(&mut app);
        app
    }};
}

fn run<V, F>(app: &mut App<V, State, F>, inputs: &[Input])
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    for &input in inputs {
        input::apply(app, input);
    }
}

fn typed(text: &str) -> Vec<Input> {
    text.chars().map(Input::Character).collect()
}

fn focus(state: &State) -> Option<FocusKey> {
    state.screen().focus().current()
}

fn draft(state: &State, chat: u32) -> &str {
    state.chat(chat).expect("chat exists").draft.text()
}

fn assert_in_conversation(state: &State, chat: u32) {
    assert!(
        matches!(state.screen(), Screen::Conversation { chat: c, .. } if *c == chat),
        "expected chat {chat}, got {:?}",
        state.screen()
    );
    assert_eq!(focus(state), Some(keys::COMPOSER));
}

#[test]
fn the_list_opens_on_its_first_chat() {
    let app = app!();
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert_eq!(focus(app.state()), Some(keys::chat(RIDGE)));
}

#[test]
fn press_and_enter_both_open_the_focused_chat() {
    let mut pressed = app!();
    run(&mut pressed, &[Input::Press]);
    assert_in_conversation(pressed.state(), RIDGE);

    let mut entered = app!();
    run(&mut entered, &[Input::Enter]);
    assert_in_conversation(entered.state(), RIDGE);
}

#[test]
fn typing_edits_at_the_cursor_and_back_keeps_the_draft() {
    let mut app = app!();
    run(&mut app, &[Input::Press]);
    // The first letter wakes the field and is typed.
    run(&mut app, &typed("!"));
    assert_eq!(
        draft(app.state(), RIDGE),
        "On my way, save me some hot coffee!"
    );
    // In an active field the wheel moves the cursor, not focus.
    run(&mut app, &[Input::Turn(-7)]);
    run(&mut app, &typed("cup of "));
    assert_eq!(
        draft(app.state(), RIDGE),
        "On my way, save me some hot cup of coffee!"
    );
    assert_eq!(focus(app.state()), Some(keys::COMPOSER));

    // Press is Done; then Backspace is Back.
    run(&mut app, &[Input::Press, Input::Backspace]);
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert_eq!(focus(app.state()), Some(keys::chat(RIDGE)));
    assert_eq!(
        draft(app.state(), RIDGE),
        "On my way, save me some hot cup of coffee!"
    );
}

#[test]
fn backspace_erases_while_typing() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Enter, Input::Backspace]);
    assert_in_conversation(app.state(), RIDGE);
    assert_eq!(
        draft(app.state(), RIDGE),
        "On my way, save me some hot coffe"
    );
}

#[test]
fn backspace_with_nothing_to_erase_ends_the_edit_then_goes_back() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(2), Input::Press, Input::Enter]);
    assert_in_conversation(app.state(), PAT);
    run(&mut app, &[Input::Backspace]);
    assert_in_conversation(app.state(), PAT);
    run(&mut app, &[Input::Backspace]);
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
}

#[test]
fn enter_sends_the_draft() {
    let mut app = app!();
    // Enter wakes the field, then sends.
    run(&mut app, &[Input::Press, Input::Enter, Input::Enter]);
    let conversation = app.state().conversation(RIDGE).expect("chat exists");
    let sent = conversation.messages.last().expect("a message");
    assert_eq!(sent.direction, Direction::Outbound);
    assert_eq!(sent.text.as_str(), RIDGE_DRAFT);
    assert_eq!(sent.delivery, Delivery::Sending);
    assert_eq!(draft(app.state(), RIDGE), "");
    assert_eq!(focus(app.state()), Some(keys::COMPOSER));

    // Sending ends the edit, so the wheel reaches the history again.
    let sent = sent.id;
    run(&mut app, &[Input::Turn(-1)]);
    assert_eq!(focus(app.state()), Some(keys::message(sent)));
}

#[test]
fn a_draft_longer_than_a_fragment_is_sent_whole() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(2), Input::Press]);
    let long = "Trail is washed out past the second creek crossing. ".repeat(6);
    assert!(long.len() > 160);
    run(&mut app, &typed(&long));
    assert_eq!(draft(app.state(), PAT), long);
    run(&mut app, &[Input::Enter]);
    let conversation = app.state().conversation(PAT).expect("chat is open");
    let sent = conversation.messages.last().expect("a message");
    assert_eq!(sent.text.as_str(), long);
}

#[test]
fn a_sent_message_is_there_when_the_chat_opens_again() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Enter, Input::Enter]);
    run(&mut app, &[Input::Backspace]);
    assert!(app.state().conversation.is_none());
    let summary = app.state().chat(RIDGE).expect("chat exists");
    assert_eq!(
        summary.preview.as_str(),
        "You: On my way, save me some hot coffee"
    );

    run(&mut app, &[Input::Press]);
    assert_in_conversation(app.state(), RIDGE);
    let conversation = app.state().conversation(RIDGE).expect("chat is open");
    let texts: Vec<&str> = conversation
        .messages
        .iter()
        .map(|m| m.text.as_str())
        .collect();
    assert_eq!(texts.len(), 4);
    assert_eq!(texts[3], RIDGE_DRAFT);
}

#[test]
fn a_chat_that_moves_to_the_top_keeps_focus() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(2), Input::Press]);
    assert_in_conversation(app.state(), PAT);
    run(&mut app, &typed("ok"));
    run(&mut app, &[Input::Enter, Input::Backspace]);
    assert_eq!(app.state().chats[0].id, PAT);
    assert_eq!(focus(app.state()), Some(keys::chat(PAT)));
}

#[test]
fn backspace_on_the_list_goes_to_the_rail_and_press_returns() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(1), Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::rail(Section::Chats)));
    run(&mut app, &[Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::chat(2)));
}

#[test]
fn open_back_and_open_again_with_no_frame_between() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Backspace, Input::Press]);
    assert_in_conversation(app.state(), RIDGE);
}

#[test]
fn typing_from_the_history_goes_to_the_composer() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Turn(-1)]);
    let reading = focus(app.state()).expect("a message is focused");
    assert!(keys::is_message(reading));
    // The composer is folded to a bar here; the letter unfolds it and lands.
    run(&mut app, &typed("k"));
    assert_in_conversation(app.state(), RIDGE);
    assert_eq!(
        draft(app.state(), RIDGE),
        "On my way, save me some hot coffeek"
    );
}

#[test]
fn backspace_in_the_history_is_back() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Turn(-2), Input::Backspace]);
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert_eq!(focus(app.state()), Some(keys::chat(RIDGE)));
}

/// Ridge Medic's messages: the question, my answer, and "See you at the hut".
const QUESTION: u32 = 0x100;
const ANSWER: u32 = 0x101;
const SEE_YOU: u32 = 0x102;

fn actions(state: &State) -> Option<u32> {
    match state.screen() {
        Screen::Conversation { actions, .. } => *actions,
        _ => None,
    }
}

fn reply(state: &State) -> Option<u32> {
    match state.screen() {
        Screen::Conversation { reply, .. } => *reply,
        _ => None,
    }
}

fn reactions(state: &State, message: u32) -> Vec<(Reaction, u8, bool)> {
    let conversation = state.conversation.as_ref().expect("a chat is open");
    let message = conversation
        .messages
        .iter()
        .find(|m| m.id == message)
        .expect("the message");
    message
        .reactions
        .iter()
        .map(|r| (r.reaction, r.count, r.mine))
        .collect()
}

#[test]
fn pressing_a_message_opens_its_actions_and_backspace_closes_them() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Turn(-1), Input::Press]);
    assert_eq!(actions(app.state()), Some(SEE_YOU));
    assert_eq!(focus(app.state()), Some(keys::REPLY));
    // The palette is above Reply.
    run(&mut app, &[Input::Turn(-1)]);
    assert_eq!(focus(app.state()), Some(keys::reaction(Reaction::Question)));
    run(&mut app, &[Input::Backspace]);
    assert_eq!(actions(app.state()), None);
    assert_eq!(focus(app.state()), Some(keys::message(SEE_YOU)));
}

#[test]
fn a_reaction_is_mine_until_chosen_again_and_kept_across_opening_the_chat() {
    let mut app = app!();
    let open_on_answer = [Input::Press, Input::Turn(-2), Input::Press, Input::Turn(-6)];
    run(&mut app, &open_on_answer);
    // Ridge Medic's heart is there already; mine joins it.
    assert_eq!(focus(app.state()), Some(keys::reaction(Reaction::Heart)));
    run(&mut app, &[Input::Press]);
    assert_eq!(actions(app.state()), None);
    assert_eq!(reactions(app.state(), ANSWER), [(Reaction::Heart, 2, true)]);

    run(&mut app, &[Input::Backspace, Input::Press]);
    assert_eq!(reactions(app.state(), ANSWER), [(Reaction::Heart, 2, true)]);

    // Choosing it again withdraws it.
    run(&mut app, &open_on_answer[1..]);
    run(&mut app, &[Input::Press]);
    assert_eq!(
        reactions(app.state(), ANSWER),
        [(Reaction::Heart, 1, false)]
    );
}

#[test]
fn typing_with_the_actions_open_goes_to_the_composer() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Turn(-1), Input::Press]);
    run(&mut app, &typed("!"));
    assert_eq!(actions(app.state()), None);
    assert_in_conversation(app.state(), RIDGE);
    assert_eq!(draft(app.state(), RIDGE), format!("{RIDGE_DRAFT}!"));
}

#[test]
fn a_reply_is_dropped_by_backspace_before_backspace_leaves() {
    let mut app = app!();
    run(
        &mut app,
        &[Input::Press, Input::Turn(-3), Input::Press, Input::Press],
    );
    assert_eq!(reply(app.state()), Some(QUESTION));
    assert_in_conversation(app.state(), RIDGE);
    // The field is not being edited: the first Backspace drops the reply
    // and keeps the text, the next leaves.
    run(&mut app, &[Input::Backspace]);
    assert_eq!(reply(app.state()), None);
    assert_in_conversation(app.state(), RIDGE);
    assert_eq!(draft(app.state(), RIDGE), RIDGE_DRAFT);
    run(&mut app, &[Input::Backspace]);
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
}

#[test]
fn sending_a_reply_ends_it() {
    let mut app = app!();
    run(
        &mut app,
        &[Input::Press, Input::Turn(-3), Input::Press, Input::Press],
    );
    run(&mut app, &[Input::Enter, Input::Enter]);
    assert_eq!(reply(app.state()), None);
    assert_eq!(draft(app.state(), RIDGE), "");
}

#[test]
fn details_open_from_the_actions_and_back_returns_to_the_message() {
    let mut app = app!();
    run(
        &mut app,
        &[
            Input::Press,
            Input::Turn(-3),
            Input::Press,
            Input::Turn(1),
            Input::Press,
        ],
    );
    let Screen::Details {
        message, details, ..
    } = app.state().screen()
    else {
        panic!("expected details, got {:?}", app.state().screen());
    };
    assert_eq!(*message, QUESTION);
    assert_eq!(details.rssi, Some(-97));
    assert_eq!(focus(app.state()), Some(keys::DETAIL_LIST));
    run(&mut app, &[Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::message(QUESTION)));
    assert_eq!(actions(app.state()), None);
}
