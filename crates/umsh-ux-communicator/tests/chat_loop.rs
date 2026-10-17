//! The chat loop driven the way a board drives it: inputs through
//! `input::apply`, with no frame drawn between them.

use buoyant::app::App;
use buoyant::focus::{DefaultFocus, FocusKey};
use buoyant::render::AnimatedJoin;
use buoyant::view::ViewLayout;
use embedded_graphics::prelude::Size;
use umsh_ux_communicator::buoyant;
use umsh_ux_communicator::input::{self, Input};
use umsh_ux_communicator::model::{
    ChannelAccess, ChannelEntry, ChatKind, Delivery, Direction, FoundKind, Notice, Preset,
    Reaction, Section, Shareable, State,
};
use umsh_ux_communicator::nav::{DevicePage, Screen, keys};
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
    assert_eq!(banner.notice, Notice::Message(TRAIL_CREW));
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

/// Jo Reyes, the chat of the fixture's last arrival.
const JO: u32 = 5;

fn set_dark<V, F>(app: &mut App<V, State, F>, dark: bool)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    app.state_mut().dark = dark;
    app.finalize_view();
}

fn arrived(state: &State) -> Vec<(u32, String)> {
    state
        .arrived
        .iter()
        .map(|a| (a.chat, a.text.as_str().to_owned()))
        .collect()
}

#[test]
fn a_message_while_dark_wakes_to_the_chats_that_got_messages() {
    let store = store();
    let mut app = app!(store);
    set_dark(&mut app, true);
    assert!(arrive(&mut app, store, 0));
    assert!(matches!(app.state().screen(), Screen::Wake { .. }));
    set_dark(&mut app, false);
    // More arrive while it shows, and join it rather than a banner.
    assert!(arrive(&mut app, store, 0));
    assert!(arrive(&mut app, store, 0));
    assert_eq!(app.state().banner, None);
    assert_eq!(
        arrived(app.state()),
        [
            (
                TRAIL_CREW,
                "Sam: Will wait 10 min at the saddle, then we head down".to_owned()
            ),
            (PAT, "Got the antenna. Bringing two.".to_owned()),
            (JO, "Are you on the ridge yet?".to_owned()),
        ]
    );
    assert_eq!(focus(app.state()), Some(keys::chat(TRAIL_CREW)));
    // Press opens the focused chat; Back from it goes to its row.
    run(&mut app, &[Input::Turn(1), Input::Press]);
    assert_in_conversation(app.state(), PAT);
    assert!(app.state().arrived.is_empty());
    run(&mut app, &[Input::Backspace]);
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert_eq!(focus(app.state()), Some(keys::chat(PAT)));
}

#[test]
fn typing_on_the_wake_screen_starts_a_reply_in_the_focused_chat() {
    let store = store();
    let mut app = app!(store);
    set_dark(&mut app, true);
    arrive(&mut app, store, 0);
    set_dark(&mut app, false);
    run(&mut app, &typed("Ok"));
    assert_in_conversation(app.state(), TRAIL_CREW);
    assert_eq!(draft(app.state(), TRAIL_CREW), "Ok");
}

#[test]
fn backspace_on_the_wake_screen_returns_to_where_it_went_dark() {
    let store = store();
    let mut app = app!(store);
    // Reading Ridge Medic when the panel goes dark, and a message there
    // wakes it: dark, nothing was being read.
    run(&mut app, &[Input::Press, Input::Turn(-1)]);
    set_dark(&mut app, true);
    let (_, message) = store.arrive().expect("an arrival");
    assert!(app.state_mut().receive(RIDGE, message, 0));
    app.finalize_view();
    set_dark(&mut app, false);
    assert!(matches!(app.state().screen(), Screen::Wake { .. }));
    assert!(app.state().chats[0].unread);
    run(&mut app, &[Input::Backspace]);
    assert!(matches!(
        app.state().screen(),
        Screen::Conversation { chat: RIDGE, .. }
    ));
    assert_eq!(focus(app.state()), Some(keys::message(SEE_YOU)));
    assert!(app.state().arrived.is_empty());
}

#[test]
fn a_muted_chat_does_not_wake_the_panel() {
    let store = store();
    let mut app = app!(store);
    set_dark(&mut app, true);
    let (_, message) = store.arrive().expect("an arrival");
    assert!(!app.state_mut().receive(4, message, 0));
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert!(app.state().arrived.is_empty());
}

#[test]
fn a_channel_send_ends_at_sent() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(1), Input::Press]);
    assert_in_conversation(app.state(), TRAIL_CREW);
    run(&mut app, &[Input::Enter]);
    run(&mut app, &typed("Switchbacks, 15 min out"));
    run(&mut app, &[Input::Enter]);
    let conversation = app.state().conversation.as_ref().expect("a chat is open");
    let sent = conversation.messages.last().expect("the message sent");
    assert_eq!(sent.text.as_str(), "Switchbacks, 15 min out");
    // Multicast has no acknowledgment to wait for.
    assert_eq!(sent.delivery, Delivery::Sent);
}

/// Opens the actions of the Trail Crew message `back` from its newest, and
/// presses View member.
fn view_member<V, F>(app: &mut App<V, State, F>, back: i8)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    run(app, &[Input::Turn(1), Input::Press]);
    assert_in_conversation(app.state(), TRAIL_CREW);
    run(app, &[Input::Turn(-back), Input::Press, Input::Turn(1)]);
    assert_eq!(focus(app.state()), Some(keys::VIEW_MEMBER));
    run(app, &[Input::Press]);
}

#[test]
fn a_member_not_yet_known_offers_to_request_their_identity() {
    let mut app = app!();
    view_member(&mut app, 2);
    let Screen::Member {
        message, member, ..
    } = app.state().screen()
    else {
        panic!("expected a member, got {:?}", app.state().screen());
    };
    let message = *message;
    assert_eq!(member.hint.as_str(), "4Fz*");
    assert_eq!(member.address, None);
    assert_eq!(focus(app.state()), Some(keys::REQUEST_IDENTITY));
    run(&mut app, &[Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::message(message)));
    assert_eq!(actions(app.state()), None);
}

#[test]
fn a_known_member_shows_their_address() {
    let mut app = app!();
    view_member(&mut app, 1);
    let Screen::Member { member, .. } = app.state().screen() else {
        panic!("expected a member, got {:?}", app.state().screen());
    };
    assert_eq!(member.sender.name.as_str(), "Sam");
    assert!(member.address.is_some());
    assert_eq!(focus(app.state()), Some(keys::MESSAGE_MEMBER));
}

#[test]
fn a_peer_message_has_no_member_to_view() {
    let mut app = app!();
    run(&mut app, &[Input::Press, Input::Turn(-3), Input::Press]);
    run(&mut app, &[Input::Turn(1)]);
    assert_eq!(focus(app.state()), Some(keys::DETAILS));
}

/// Member 4Fz*'s message, which others have reacted to with 👍 and ‼.
const BEHIND_YOU: u32 = (TRAIL_CREW << 8) | 1;

#[test]
fn changing_my_reaction_moves_it_between_shared_chips() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(1), Input::Press, Input::Turn(-2)]);
    assert_eq!(focus(app.state()), Some(keys::message(BEHIND_YOU)));
    let on = |reaction: Reaction| {
        let step = Reaction::ALL
            .iter()
            .position(|&r| r == reaction)
            .expect("in the palette");
        [
            Input::Press,
            Input::Turn(-(Reaction::ALL.len() as i8 - step as i8)),
            Input::Press,
        ]
    };
    run(&mut app, &on(Reaction::ThumbsUp));
    assert_eq!(
        reactions(app.state(), BEHIND_YOU),
        [(Reaction::ThumbsUp, 2, true), (Reaction::Exclaim, 1, false)]
    );
    run(&mut app, &on(Reaction::Exclaim));
    assert_eq!(
        reactions(app.state(), BEHIND_YOU),
        [(Reaction::ThumbsUp, 1, false), (Reaction::Exclaim, 2, true)]
    );
}

fn shown_member(state: &State) -> (&umsh_ux_communicator::model::Member, bool) {
    match state.screen() {
        Screen::Member {
            member, requested, ..
        } => (member, *requested),
        other => panic!("expected a member, got {other:?}"),
    }
}

#[test]
fn an_identity_that_arrives_names_the_member_everywhere() {
    let store = store();
    let mut app = app!(store);
    view_member(&mut app, 2);
    assert!(!store.answer_identity(), "nothing asked yet");
    run(&mut app, &[Input::Press]);
    assert!(shown_member(app.state()).1, "requested");
    assert!(store.answer_identity());
    app.state_mut().identities_arrived();
    app.finalize_view();

    let (member, _) = shown_member(app.state());
    assert_eq!(member.sender.name.as_str(), "Robin Hale");
    assert_eq!(member.hint.as_str(), "4FzR");
    assert!(member.address.is_some());
    assert_eq!(focus(app.state()), Some(keys::MESSAGE_MEMBER));
    let conversation = app.state().conversation.as_ref().expect("a chat is open");
    let sender = conversation.messages.iter().find(|m| m.id == BEHIND_YOU);
    assert_eq!(
        sender
            .and_then(|m| m.sender.as_ref())
            .map(|s| s.name.as_str()),
        Some("Robin Hale")
    );
}

#[test]
fn save_peer_goes_once_saved_and_message_opens_a_direct_chat() {
    let mut app = app!();
    view_member(&mut app, 1);
    run(&mut app, &[Input::Turn(1)]);
    assert_eq!(focus(app.state()), Some(keys::SAVE_PEER));
    run(&mut app, &[Input::Press]);
    assert!(shown_member(app.state()).0.saved);
    assert_eq!(
        app.state().peers.first().map(|p| p.name.as_str()),
        Some("Sam")
    );
    assert_eq!(focus(app.state()), Some(keys::MESSAGE_MEMBER));

    run(&mut app, &[Input::Press]);
    let chat = match app.state().screen() {
        Screen::Conversation { chat, .. } => *chat,
        other => panic!("expected a conversation, got {other:?}"),
    };
    assert_eq!(app.state().chat(chat).map(|c| c.name.as_str()), Some("Sam"));
    assert_eq!(app.state().chats.first().map(|c| c.id), Some(chat));
    assert_in_conversation(app.state(), chat);
    // Back goes to the list, at the new chat's row.
    run(&mut app, &[Input::Backspace]);
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert_eq!(focus(app.state()), Some(keys::chat(chat)));
}

/// Jo's invitation to Base Camp.
const INVITE: u32 = (JO << 8) | 2;

/// Opens Jo's chat, `row` down the list, and turns up to the invitation, through the tall
/// message, which the wheel scrolls through first.
fn to_invite<V, F>(app: &mut App<V, State, F>, row: i8)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    run(app, &[Input::Turn(row), Input::Press]);
    assert_in_conversation(app.state(), JO);
    for _ in 0..20 {
        if focus(app.state()) == Some(keys::message(INVITE)) {
            return;
        }
        run(app, &[Input::Turn(-1)]);
    }
    panic!("never reached the invitation: {:?}", focus(app.state()));
}

fn joined_chat(state: &State) -> u32 {
    match state.screen() {
        Screen::Conversation { chat, .. } => *chat,
        other => panic!("expected a conversation, got {other:?}"),
    }
}

#[test]
fn an_invitation_offers_joining_first_and_join_opens_the_channel() {
    let mut app = app!();
    to_invite(&mut app, 4);
    run(&mut app, &[Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::JOIN));
    run(&mut app, &[Input::Press]);
    assert!(matches!(app.state().screen(), Screen::Join { .. }));
    assert_eq!(focus(app.state()), Some(keys::JOIN_CHANNEL));
    // Nothing is joined until Join channel.
    let before = app.state().chats.len();
    run(&mut app, &[Input::Backspace]);
    assert_eq!(app.state().chats.len(), before);
    assert_eq!(focus(app.state()), Some(keys::message(INVITE)));

    run(&mut app, &[Input::Press, Input::Press, Input::Press]);
    let chat = joined_chat(app.state());
    assert_in_conversation(app.state(), chat);
    let summary = app.state().chat(chat).expect("joined");
    assert_eq!(summary.name.as_str(), "Base Camp");
    assert_eq!(
        summary.kind,
        ChatKind::Channel {
            access: ChannelAccess::Private
        }
    );
    assert_eq!(app.state().chats.first().map(|c| c.id), Some(chat));

    // Joining again opens the same channel.
    run(&mut app, &[Input::Backspace]);
    // The channel heads the list now, above Jo.
    run(&mut app, &[Input::Turn(-20)]);
    to_invite(&mut app, 5);
    run(&mut app, &[Input::Press, Input::Press, Input::Press]);
    assert_eq!(joined_chat(app.state()), chat);
    assert_eq!(app.state().chats.len(), before + 1);
}

#[test]
fn a_channel_joined_can_be_called_something_else_here() {
    let mut app = app!();
    to_invite(&mut app, 4);
    run(&mut app, &[Input::Press, Input::Press, Input::Turn(-1)]);
    assert_eq!(focus(app.state()), Some(keys::JOIN_NAME));
    run(&mut app, &[Input::Press]);
    run(&mut app, &[Input::Backspace; 4]);
    run(&mut app, &typed("Hut"));
    run(&mut app, &[Input::Enter, Input::Turn(1), Input::Press]);
    let chat = joined_chat(app.state());
    assert_eq!(
        app.state().chat(chat).map(|c| c.name.as_str()),
        Some("Base Hut")
    );
}

const EMERGENCY: u32 = 6;

#[test]
fn emergency_is_open_and_its_sends_end_at_sent() {
    let mut app = app!();
    run(&mut app, &[Input::Turn(5), Input::Press]);
    assert_in_conversation(app.state(), EMERGENCY);
    let kind = app
        .state()
        .chat(EMERGENCY)
        .expect("joined from the start")
        .kind;
    assert_eq!(
        kind,
        ChatKind::Channel {
            access: ChannelAccess::Clear
        }
    );
    assert!(kind.is_open());
    run(&mut app, &[Input::Enter]);
    run(&mut app, &typed("Heading down"));
    run(&mut app, &[Input::Enter]);
    let conversation = app.state().conversation.as_ref().expect("a chat is open");
    assert_eq!(
        conversation.messages.last().map(|m| m.delivery),
        Some(Delivery::Sent)
    );
}

#[test]
fn the_rail_switches_to_peers_which_open_on_add_peer() {
    let mut app = app!();
    run(&mut app, &[Input::Backspace, Input::Turn(1)]);
    assert_eq!(focus(app.state()), Some(keys::rail(Section::Peers)));
    run(&mut app, &[Input::Press]);
    assert_eq!(app.state().section, Section::Peers);
    assert_eq!(focus(app.state()), Some(keys::ADD_PEER));
    run(&mut app, &[Input::Turn(1)]);
    let first = app.state().peers.first().map(|p| keys::peer(p.id));
    assert_eq!(focus(app.state()), first);
    // Back on the rail, and into Chats, at its first chat.
    run(&mut app, &[Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::rail(Section::Peers)));
    run(&mut app, &[Input::Turn(-1), Input::Press]);
    assert_eq!(app.state().section, Section::Chats);
    assert_eq!(focus(app.state()), Some(keys::chat(RIDGE)));
}

#[test]
fn add_peer_unlocks_message_once_the_address_is_whole() {
    use umsh_ux_communicator::model::PeerEntry;
    let mut app = app!();
    run(
        &mut app,
        &[Input::Backspace, Input::Turn(1), Input::Press, Input::Press],
    );
    assert!(matches!(app.state().screen(), Screen::AddPeer { .. }));
    assert_eq!(focus(app.state()), Some(keys::PEER_FIELD));
    run(&mut app, &typed("7cQRwjEyMJRDQCrtkgvaKiXfJ2CrUi"));
    assert!(matches!(
        app.state().peer_entry(),
        PeerEntry::Partial {
            avatar: Some(_),
            missing: 14,
            ..
        }
    ));
    // Not whole: Enter only ends the edit, and Message cannot be reached.
    run(&mut app, &[Input::Enter, Input::Turn(1)]);
    assert!(matches!(app.state().screen(), Screen::AddPeer { .. }));
    assert_ne!(focus(app.state()), Some(keys::MESSAGE_PEER));

    run(&mut app, &[Input::Turn(-1)]);
    run(&mut app, &typed("8pTn4kWsYbHd9e"));
    assert!(matches!(
        app.state().peer_entry(),
        PeerEntry::Complete { .. }
    ));
    run(&mut app, &[Input::Enter]);
    let chat = joined_chat(app.state());
    assert_in_conversation(app.state(), chat);
    assert_eq!(
        app.state().chat(chat).map(|c| c.name.as_str()),
        Some("7cQR")
    );
    assert_eq!(app.state().section, Section::Chats);
}

#[test]
fn add_peer_reads_an_invitation_and_refuses_what_is_not_an_address() {
    use umsh_ux_communicator::model::PeerEntry;
    let mut app = app!();
    run(
        &mut app,
        &[Input::Backspace, Input::Turn(1), Input::Press, Input::Press],
    );
    run(
        &mut app,
        &typed("umsh:n:7cQRwjEyMJRDQCrtkgvaKiXfJ2CrUi8pTn4kWsYbHd9e"),
    );
    assert!(matches!(
        app.state().peer_entry(),
        PeerEntry::Complete { .. }
    ));
    run(&mut app, &[Input::Backspace; 51]);
    run(&mut app, &typed("0O"));
    assert_eq!(app.state().peer_entry(), PeerEntry::Invalid);
}

fn peer_detail(state: &State) -> &umsh_ux_communicator::model::PeerDetail {
    match state.screen() {
        Screen::Peer { detail, .. } => detail,
        other => panic!("expected a peer, got {other:?}"),
    }
}

/// Opens the first saved peer, Pat Okafor, from the Peers list.
fn open_pat<V, F>(app: &mut App<V, State, F>)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    run(
        app,
        &[
            Input::Backspace,
            Input::Turn(1),
            Input::Press,
            Input::Turn(1),
            Input::Press,
        ],
    );
    assert_eq!(focus(app.state()), Some(keys::PEER_MESSAGE));
}

#[test]
fn a_saved_peer_shows_its_detail_and_messages_in_its_own_chat() {
    let mut app = app!();
    open_pat(&mut app);
    assert!(peer_detail(app.state()).address.starts_with("YmRG"));
    run(&mut app, &[Input::Press]);
    assert_in_conversation(app.state(), PAT);
}

#[test]
fn forward_secrecy_switches_and_a_ping_is_answered() {
    let mut app = app!();
    open_pat(&mut app);
    run(&mut app, &[Input::Turn(3)]);
    assert_eq!(focus(app.state()), Some(keys::PEER_SECRECY));
    run(&mut app, &[Input::Press]);
    assert!(peer_detail(app.state()).forward_secrecy);
    run(&mut app, &[Input::Press]);
    assert!(!peer_detail(app.state()).forward_secrecy);
    run(&mut app, &[Input::Turn(1), Input::Press]);
    assert_eq!(peer_detail(app.state()).ping.as_deref(), Some("1.4 s"));
}

#[test]
fn forget_asks_first_and_cancel_keeps_the_peer() {
    let mut app = app!();
    open_pat(&mut app);
    let peers = app.state().peers.len();
    run(&mut app, &[Input::Turn(5)]);
    assert_eq!(focus(app.state()), Some(keys::PEER_FORGET));
    run(&mut app, &[Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::FORGET_CANCEL));
    run(&mut app, &[Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::PEER_FORGET));
    assert_eq!(app.state().peers.len(), peers);

    run(&mut app, &[Input::Press, Input::Turn(-1), Input::Press]);
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert_eq!(app.state().peers.len(), peers - 1);
    // The row that took its place is focused.
    let next = app.state().peers.first().map(|p| keys::peer(p.id));
    assert_eq!(focus(app.state()), next);
}

fn radio(state: &State) -> (&umsh_ux_communicator::model::RadioSettings, bool) {
    match state.screen() {
        Screen::Device {
            page: DevicePage::Radio { settings, picker },
            ..
        } => (settings, *picker),
        other => panic!("expected Radio, got {other:?}"),
    }
}

/// Opens Device › Radio from the Chats list.
fn open_radio<V, F>(app: &mut App<V, State, F>)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    run(app, &[Input::Backspace, Input::Turn(2), Input::Press]);
    assert_eq!(app.state().section, Section::Device);
    assert_eq!(focus(app.state()), Some(keys::IDENTITY));
    run(app, &[Input::Turn(2), Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::REGION));
}

#[test]
fn a_preset_is_chosen_from_its_choices_and_backspace_cancels() {
    let mut app = app!();
    open_radio(&mut app);
    run(&mut app, &[Input::Turn(1), Input::Press]);
    assert!(radio(app.state()).1);
    assert_eq!(focus(app.state()), Some(keys::preset(Preset::LongRange)));
    run(&mut app, &[Input::Turn(1), Input::Backspace]);
    assert!(!radio(app.state()).1);
    assert_eq!(radio(app.state()).0.preset, Preset::LongRange);
    assert_eq!(focus(app.state()), Some(keys::PRESET_ROW));

    run(&mut app, &[Input::Press, Input::Turn(1), Input::Press]);
    assert_eq!(radio(app.state()).0.preset, Preset::MediumRange);
    assert!(!radio(app.state()).1);
    assert_eq!(focus(app.state()), Some(keys::PRESET_ROW));

    // Back returns to Radio's category, and again to the rail.
    run(&mut app, &[Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::RADIO));
    run(&mut app, &[Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::rail(Section::Device)));
}

#[test]
fn a_switch_flips_on_press() {
    let mut app = app!();
    open_radio(&mut app);
    run(&mut app, &[Input::Turn(3)]);
    assert_eq!(focus(app.state()), Some(keys::SHARE_LOCATION));
    assert!(!radio(app.state()).0.share_location);
    run(&mut app, &[Input::Press]);
    assert!(radio(app.state()).0.share_location);
    run(&mut app, &[Input::Turn(-1), Input::Press]);
    assert!(!radio(app.state()).0.forwarding);
}

#[test]
fn turning_on_share_moves_between_this_node_and_its_channels() {
    let mut app = app!();
    run(
        &mut app,
        &[Input::Backspace, Input::Turn(2), Input::Press, Input::Press],
    );
    assert!(matches!(
        app.state().screen(),
        Screen::Device {
            page: DevicePage::Share,
            ..
        }
    ));
    assert_eq!(app.state().shared(), Shareable::Identity);
    run(&mut app, &[Input::Turn(1)]);
    assert_eq!(app.state().shared(), Shareable::Channel(2));
    run(&mut app, &[Input::Turn(2)]);
    assert_eq!(app.state().shared(), Shareable::Channel(6));
    // The last stays put.
    run(&mut app, &[Input::Turn(1)]);
    assert_eq!(app.state().shared(), Shareable::Channel(6));
    run(&mut app, &[Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::IDENTITY));
}

/// Opens Device › Channels › Add channel from the Chats list.
fn open_add_channel<V, F>(app: &mut App<V, State, F>)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    run(
        app,
        &[
            Input::Backspace,
            Input::Turn(2),
            Input::Press,
            Input::Turn(1),
            Input::Press,
        ],
    );
    assert_eq!(focus(app.state()), Some(keys::ADD_CHANNEL));
    run(app, &[Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::CHANNEL_FIELD));
}

#[test]
fn a_channel_name_joins_a_named_channel() {
    let mut app = app!();
    open_add_channel(&mut app);
    assert_eq!(app.state().channel_entry(), ChannelEntry::Empty);
    run(&mut app, &typed("Ridge Runners"));
    assert_eq!(app.state().channel_entry(), ChannelEntry::Named);
    run(&mut app, &[Input::Enter]);
    let chat = app.state().chats[0].id;
    assert_eq!(app.state().chats[0].name.as_str(), "Ridge Runners");
    assert_eq!(
        app.state().chats[0].kind,
        ChatKind::Channel {
            access: ChannelAccess::Named
        }
    );
    assert_in_conversation(app.state(), chat);
}

#[test]
fn an_invitation_counts_down_and_joins_a_private_channel() {
    let mut app = app!();
    open_add_channel(&mut app);
    run(
        &mut app,
        &typed("umsh:ck:5BFn8YGKJ6pZR4qV3tW7mNhDrXsCxEaL9kUv2wAjT"),
    );
    assert_eq!(
        app.state().channel_entry(),
        ChannelEntry::Partial { missing: 3 }
    );
    // Join is not there to press yet.
    run(&mut app, &[Input::Enter, Input::Turn(1)]);
    assert_eq!(focus(app.state()), Some(keys::CHANNEL_FIELD));
    run(&mut app, &[Input::Press]);
    run(&mut app, &typed("8bP?n=Base%20Camp"));
    assert_eq!(app.state().channel_entry(), ChannelEntry::Private);
    run(&mut app, &[Input::Enter]);
    assert_eq!(app.state().chats[0].name.as_str(), "Base Camp");
    assert_in_conversation(app.state(), app.state().chats[0].id);
}

#[test]
fn a_joined_channel_opens_from_the_channels_list() {
    let mut app = app!();
    open_add_channel(&mut app);
    run(&mut app, &[Input::Backspace, Input::Turn(1), Input::Press]);
    // Trail Crew, the first channel in the list.
    assert_in_conversation(app.state(), 2);
}

#[test]
fn low_battery_says_so_once_without_taking_a_key() {
    let mut app = app!();
    // Typing in Ridge Medic.
    run(&mut app, &[Input::Press, Input::Enter]);
    app.state_mut().battery_low(9, 1000);
    app.finalize_view();
    assert!(app.state().status.battery_low);
    let banner = app.state().banner.expect("a banner");
    assert_eq!(banner.notice, Notice::BatteryLow { percent: 9 });
    run(&mut app, &typed("!"));
    assert!(draft(app.state(), RIDGE).ends_with('!'));
    app.state_mut().expire(banner.until);
    assert_eq!(app.state().banner, None);
    // The strip keeps saying it.
    assert!(app.state().status.battery_low);
}

#[test]
fn low_battery_while_dark_marks_only_the_strip() {
    let mut app = app!();
    app.state_mut().dark = true;
    app.state_mut().battery_low(9, 1000);
    assert!(app.state().status.battery_low);
    assert_eq!(app.state().banner, None);
}

#[test]
fn first_run_takes_a_name_then_a_region_and_ends_on_the_chats() {
    let mut app = app!();
    app.state_mut().start_setup();
    app.finalize_view();
    assert!(app.state().in_setup());
    assert_eq!(focus(app.state()), Some(keys::NAME_FIELD));
    // Backspace has nothing to go back to.
    run(&mut app, &[Input::Backspace]);
    assert!(app.state().in_setup());
    run(&mut app, &typed("Wren"));
    run(&mut app, &[Input::Enter]);
    assert_eq!(app.state().status.device_name.as_str(), "Wren");
    assert_eq!(focus(app.state()), Some(keys::region(0)));
    // Back to the name, and on again.
    run(&mut app, &[Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::NAME_FIELD));
    run(&mut app, &[Input::Turn(1), Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::region(0)));
    run(&mut app, &[Input::Turn(1), Input::Press]);
    assert!(!app.state().in_setup());
    assert!(matches!(app.state().screen(), Screen::Chats { .. }));
    assert_eq!(app.state().node.radio().region.as_str(), "Canada");
}

#[test]
fn the_region_is_chosen_from_radio_too() {
    let mut app = app!();
    open_radio(&mut app);
    run(&mut app, &[Input::Press]);
    assert_eq!(focus(app.state()), Some(keys::region(0)));
    run(&mut app, &[Input::Turn(2), Input::Press]);
    assert_eq!(radio(app.state()).0.region.as_str(), "European Union");
    assert_eq!(focus(app.state()), Some(keys::REGION));
}

#[test]
fn typing_on_the_list_finds_and_enter_opens_the_top_result() {
    let mut app = app!();
    run(&mut app, &typed("ri"));
    assert_eq!(focus(app.state()), Some(keys::FIND_FIELD));
    assert_eq!(app.state().find_query(), Some("ri"));
    let (found, count) = app.state().found();
    // Ridge Medic; Rick Alvarez; Riverbend Camp and Ridge Runner, heard.
    assert_eq!(count, 4);
    assert_eq!(found[0].kind, FoundKind::Chat(RIDGE));
    assert_eq!(found[1].kind, FoundKind::Peer(app.state().peers[2].id));
    assert_eq!(found[2].name.as_str(), "Riverbend Camp");
    run(&mut app, &[Input::Enter]);
    assert_in_conversation(app.state(), RIDGE);
    // Find closed behind it.
    run(&mut app, &[Input::Backspace]);
    assert_eq!(app.state().find_query(), None);
    assert_eq!(focus(app.state()), Some(keys::chat(RIDGE)));
}

#[test]
fn backspace_ends_find_and_turning_moves_into_the_results() {
    let mut app = app!();
    run(&mut app, &typed("cr"));
    // Trail Crew matches at its second word.
    assert_eq!(app.state().found().0[0].kind, FoundKind::Chat(2));
    // Press ends the edit; turning then moves into the results.
    run(&mut app, &[Input::Press, Input::Turn(1)]);
    assert_eq!(focus(app.state()), Some(keys::found(0)));
    run(&mut app, &[Input::Press]);
    assert_in_conversation(app.state(), 2);
    run(&mut app, &[Input::Backspace]);
    // Typing again and erasing it all, then Backspace closes Find.
    run(&mut app, &typed("x"));
    assert_eq!(app.state().found().1, 0);
    run(&mut app, &[Input::Backspace, Input::Backspace]);
    assert_eq!(focus(app.state()), Some(keys::FIND_FIELD));
    run(&mut app, &[Input::Backspace]);
    assert_eq!(app.state().find_query(), None);
    assert_eq!(focus(app.state()), Some(keys::chat(RIDGE)));
}
