use cursive::backends::crossterm::crossterm::style::Color;
use cursive::traits::Nameable;
use cursive::utils::markup::StyledString;
use cursive::views::{LinearLayout, NamedView, Panel, ResizedView, ScrollView, TextView};
use cursive::Cursive;

use crate::error::ArpchatError;
use crate::net::{EtherType, Id};

pub enum UpdatePresenceKind {
    Boring,
    JoinOrReconnect,
    UsernameChange(String),
}

pub enum UICommand {
    AlertUser,
    UpdateUsername(String),
    SendMessage(String),
    SetInterface(String),
    SetEtherType(EtherType),
    NewMessage(Id, String, String, bool),
    PresenceUpdate(Id, String, bool, UpdatePresenceKind),
    RemovePresence(Id, String),
    Error(ArpchatError),
}

pub enum NetCommand {
    UpdateUsername(String),
    SendMessage(String),
    SetInterface(String),
    SetEtherType(EtherType),
    PauseHeartbeat(bool),
    Terminate,
}

// AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
type ChatPanel = Panel<ScrollView<ResizedView<ResizedView<NamedView<LinearLayout>>>>>;

pub fn update_title(siv: &mut Cursive, username: &str, interface: &str) {
    let title = if interface.len() <= 8 {
        format!("arpchat: {username} ({interface})")
    } else {
        format!("arpchat: {username}")
    };
    siv.set_window_title(&title);
    siv.call_on_name("chat_panel", |chat_panel: &mut ChatPanel| {
        chat_panel.set_title(title);
    });
}

/// If a `TextView` with the provided name exists, set its content. Otherwise,
/// append a new `TextView` to the `LinearLayout` with the provided parent name.
pub fn update_or_append_txt<S>(siv: &mut Cursive, parent_id: &str, id: &str, content: S)
where
    S: Into<StyledString> + Clone,
{
    let mut updated = false;
    siv.call_on_name(id, |child: &mut TextView| {
        child.set_content(content.clone());
        updated = true;
    });

    if !updated {
        siv.call_on_name(parent_id, |parent: &mut LinearLayout| {
            parent.add_child(TextView::new(content).with_name(id));
        });
    }
}

/// Append a new `TextView` to the `LinearLayout` with the provided parent name.
pub fn append_txt<S>(siv: &mut Cursive, parent_id: &str, content: S)
where
    S: Into<StyledString>,
{
    siv.call_on_name(parent_id, |parent: &mut LinearLayout| {
        parent.add_child(TextView::new(content));
    });
}

pub fn color_from_id(id: &Id) -> Color {
    const COLOR_COUNT: usize = 8;
    const COLORS: [Color; COLOR_COUNT] = [
        Color::Red,
        Color::DarkRed,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::White,
    ];

    let index = id
        .iter()
        .copied()
        .reduce(|acc, el| acc.overflowing_add(el).0)
        .unwrap();
    let index = (index as usize) % COLOR_COUNT;
    COLORS[index]
}

pub fn ring_bell() {
    use std::io::{stdout, Write};
    print!("\x07");
    let _ = stdout().flush();
}
