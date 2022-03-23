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
    UpdateUsername(String),
    SendMessage(String),
    SetInterface(String),
    SetEtherType(EtherType),
    NewMessage {
        id: u64,
        // My goal in life is to make everyone use as many owned strings as possible
        username: String,
        channel: String,
        message: String,
    },
    PresenceUpdate(Id, String, bool, UpdatePresenceKind),
    RemovePresence(Id, String),
    Error(ArpchatError),
    SetChannel(String),
    Reaction(u64, char),
}

pub enum NetCommand {
    UpdateUsername(String),
    SendMessage(String, String),
    SetInterface(String),
    SetEtherType(EtherType),
    PauseHeartbeat(bool),
    Reaction(u64, char),
    Terminate,
}

pub fn update_title(siv: &mut Cursive, username: &str, interface: &str, channel: &str) {
    let channel = if channel.is_empty() {
        "<no chan.>"
    } else {
        channel
    };

    let title = if interface.len() <= 8 {
        format!("arpchat: {username} (#{channel}) ({interface})")
    } else {
        format!("arpchat: {username} (#{channel})")
    };
    siv.set_window_title(&title);
    siv.call_on_name(
        "chat_panel",
        // AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
        |chat_panel: &mut Panel<ScrollView<ResizedView<ResizedView<NamedView<LinearLayout>>>>>| {
            chat_panel.set_title(title);
        },
    );
}

/// If `TextView`s with the provided name exist, set their content. Otherwise,
/// append a new `TextView` to the `LinearLayout` with the provided parent name.
pub fn update_or_append_txt<S>(siv: &mut Cursive, parent_id: &str, id: &str, content: S)
where
    S: Into<StyledString> + Clone,
{
    let mut updated = false;
    siv.call_on_all_named(id, |child: &mut TextView| {
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

pub fn clear_children(siv: &mut Cursive, id: &str) {
    siv.call_on_name(id, |element: &mut LinearLayout| {
        element.clear();
    });
}
