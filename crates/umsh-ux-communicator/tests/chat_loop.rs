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

/// Where the fixture's chats keep their messages, for the life of a test.
fn store() -> &'static fixture::Store {
    Box::leak(Box::new(fixture::Store::new(&std::alloc::System)))
}

macro_rules! app {
    () => {
        app!(store())
    };
    ($store:expr) => {{
        let mut app = App::new(
            fixture::state($store),
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

/// Pat Okafor's last message, which went unconfirmed.
const ANTENNA: u32 = (PAT << 8) | 2;

fn delivery(state: &State, message: u32) -> Delivery {
    let conversation = state.conversation.as_ref().expect("a chat is open");
    conversation
        .messages
        .iter()
        .find(|m| m.id == message)
        .expect("the message")
        .delivery
}

fn spend_airtime<V, F>(app: &mut App<V, State, F>, wait: Option<u16>)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    app.state_mut().airtime_wait = wait;
    app.finalize_view();
}

#[test]
fn pressing_an_unconfirmed_message_sends_it_again() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(2), Input::Press]);
    assert_in_conversation(app.state(), PAT);
    run(&mut app, &[Input::Turn(-1)]);
    assert_eq!(focus(app.state()), Some(keys::message(ANTENNA)));
    assert_eq!(delivery(app.state(), ANTENNA), Delivery::Unconfirmed);
    run(&mut app, &[Input::Press]);
    assert_eq!(delivery(app.state(), ANTENNA), Delivery::Sending);
    assert_eq!(actions(app.state()), None);
    // On its way again, it is a message like any other.
    run(&mut app, &[Input::Press]);
    assert_eq!(actions(app.state()), Some(ANTENNA));
    // And it stays on its way when the chat opens again.
    run(
        &mut app,
        &[Input::Backspace, Input::Backspace, Input::Press],
    );
    assert_eq!(delivery(app.state(), ANTENNA), Delivery::Sending);
}

#[test]
fn spent_airtime_keeps_the_draft_and_holds_a_retry() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(2), Input::Press]);
    spend_airtime(&mut app, Some(100));
    run(&mut app, &typed("Also the 18650s"));
    run(&mut app, &[Input::Enter]);
    assert_eq!(draft(app.state(), PAT), "Also the 18650s");
    assert_eq!(app.state().conversation.as_ref().unwrap().messages.len(), 3);
    run(&mut app, &[Input::Turn(-1), Input::Press]);
    assert_eq!(delivery(app.state(), ANTENNA), Delivery::Unconfirmed);

    spend_airtime(&mut app, None);
    run(&mut app, &[Input::Press]);
    assert_eq!(delivery(app.state(), ANTENNA), Delivery::Sending);
    // The first Enter takes up the draft again; the second sends it.
    run(&mut app, &[Input::Turn(1), Input::Enter, Input::Enter]);
    assert_eq!(draft(app.state(), PAT), "");
}

/// Trail Crew, the chat the fixture's first arrival comes in.
const TRAIL_CREW: u32 = 2;

/// Takes in the fixture's next message to arrive, at `now`, as a board
/// would. Returns whether it calls for attention.
fn arrive<V, F>(app: &mut App<V, State, F>, store: &fixture::Store, now: u64) -> bool
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    let (chat, message) = store.arrive().expect("an arrival left");
    let noticed = app.state_mut().receive(chat, message, now);
    app.finalize_view();
    noticed
}

#[test]
fn a_message_from_elsewhere_shows_a_banner_that_takes_no_key() {
    let store = store();
    let mut app = app!(store);
    // Typing in Ridge Medic, between "hot" and " coffee".
    run(&mut app, &[Input::Press, Input::Enter, Input::Turn(-7)]);
    assert!(arrive(&mut app, store, 1000));
    let banner = app.state().banner.expect("a banner");
    assert_eq!(banner.chat, TRAIL_CREW);
    assert_eq!(app.state().chats[0].id, TRAIL_CREW);
    assert!(app.state().chats[0].unread);
    // Typing goes on where it was.
    run(&mut app, &typed("ed"));
    assert_eq!(
        draft(app.state(), RIDGE),
        "On my way, save me some hoted coffee"
    );
    // And it goes by itself.
    app.state_mut().expire(banner.until - 1);
    assert!(app.state().banner.is_some());
    app.state_mut().expire(banner.until);
    assert_eq!(app.state().banner, None);
    // Enter still sends the draft it interrupted.
    run(&mut app, &[Input::Enter]);
    assert_eq!(draft(app.state(), RIDGE), "");
}

#[test]
fn a_message_in_the_open_chat_joins_it_without_a_banner() {
    let store = store();
    let mut app = app!(store);
    run(&mut app, &[Input::Turn(1), Input::Press]);
    assert_in_conversation(app.state(), TRAIL_CREW);
    let before = app.state().conversation.as_ref().unwrap().messages.len();
    assert!(!arrive(&mut app, store, 0));
    assert_eq!(app.state().banner, None);
    assert!(!app.state().chats[0].unread);
    let conversation = app.state().conversation.as_ref().unwrap();
    assert_eq!(conversation.messages.len(), before + 1);
    assert_eq!(
        conversation.messages.last().unwrap().direction,
        Direction::Inbound
    );
}

#[test]
fn the_list_shows_a_message_arrive_without_a_banner() {
    let store = store();
    let mut app = app!(store);
    assert!(arrive(&mut app, store, 0));
    assert_eq!(app.state().banner, None);
    assert_eq!(app.state().chats[0].id, TRAIL_CREW);
    // Focus stays on the chat it was on.
    assert_eq!(focus(app.state()), Some(keys::chat(RIDGE)));
}

#[test]
fn a_muted_chat_does_not_call_for_attention() {
    let store = store();
    let mut app = app!(store);
    run(&mut app, &[Input::Press]);
    // The public channel is muted.
    let (_, message) = store.arrive().expect("an arrival");
    assert!(!app.state_mut().receive(4, message, 0));
    assert_eq!(app.state().banner, None);
    assert_eq!(app.state().chats[0].id, 4);
}
