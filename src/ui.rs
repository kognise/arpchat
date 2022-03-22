use std::collections::HashMap;
use std::thread;

use crossbeam_channel::{unbounded, Receiver, Sender};
use cursive::backends::crossterm::crossterm::style::Stylize;
use cursive::direction::Direction;
use cursive::event::Key;
use cursive::traits::{Nameable, Resizable, Scrollable};
use cursive::view::ScrollStrategy;
use cursive::views::{
    Dialog, EditView, LinearLayout, NamedView, Panel, ResizedView, ScrollView, SelectView, TextView,
};
use cursive::{Cursive, View};
use rand::Rng;

use crate::error::ArpchatError;
use crate::net::{sorted_usable_interfaces, Channel, Id, Packet};

enum UICommand {
    UpdateUsername(String),
    SendMessage(String),
    SetInterface(String),
    NewMessage(String, Id, String),
    NewPresence(Id, bool, String),
    UpdatePresence(Id, String, String),
    RemovePresence(Id, String),
    Error(ArpchatError),
}

enum NetCommand {
    UpdateUsername(String),
    SendMessage(String),
    SetInterface(String),
    Terminate,
}

fn init_app(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
    siv.menubar()
        .add_leaf("set username", {
            let ui_tx = ui_tx.clone();
            move |siv| show_username_dialog(siv, ui_tx.clone(), false)
        })
        .add_leaf("quit", |siv| siv.quit());
    siv.set_autohide_menu(false);
    siv.add_global_callback(Key::Esc, |siv| siv.select_menubar());

    siv.add_fullscreen_layer(
        LinearLayout::horizontal()
            .child(
                LinearLayout::vertical()
                    .child(
                        Panel::new(
                            LinearLayout::vertical()
                                .with_name("chat_inner")
                                .full_height()
                                .full_width()
                                .scrollable()
                                .scroll_strategy(ScrollStrategy::StickToBottom),
                        )
                        .title("arpchat")
                        .with_name("chat_panel")
                        .full_height()
                        .full_width(),
                    )
                    .child(
                        Panel::new(
                            EditView::new()
                                .on_submit(move |siv, msg| {
                                    siv.call_on_name("input", |input: &mut EditView| {
                                        input.set_content("");
                                    });
                                    ui_tx.send(UICommand::SendMessage(msg.to_string())).unwrap();
                                })
                                .with_name("input"),
                        )
                        .full_width(),
                    )
                    .full_width(),
            )
            .child(
                Panel::new(
                    LinearLayout::vertical()
                        .with_name("presences")
                        .full_height()
                        .full_width()
                        .scrollable()
                        .scroll_strategy(ScrollStrategy::StickToBottom),
                )
                .title("online users")
                .full_height()
                .fixed_width(32),
            ),
    );
}

fn show_iface_dialog(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
    siv.add_layer(
        Dialog::new()
            .title("select an interface")
            .content(
                SelectView::new()
                    .with_all(sorted_usable_interfaces().into_iter().map(|iface| {
                        (
                            format!(
                                "{} - {}",
                                if iface.description.is_empty() {
                                    &iface.name
                                } else {
                                    &iface.description
                                },
                                iface.mac.unwrap(),
                            ),
                            iface.name,
                        )
                    }))
                    .on_submit(move |siv, name: &String| {
                        ui_tx.send(UICommand::SetInterface(name.clone())).unwrap();
                        siv.pop_layer();
                        show_username_dialog(siv, ui_tx.clone(), true);
                    })
                    .with_name("iface_select"),
            )
            .min_width(32),
    );
}

fn show_username_dialog(siv: &mut Cursive, ui_tx: Sender<UICommand>, init_after: bool) {
    if let Some(ref mut username_dialog) = siv.find_name::<Dialog>("username_dialog") {
        username_dialog.take_focus(Direction::none()).unwrap();
        return;
    }

    siv.add_layer(
        Dialog::new()
            .title("set username")
            .content(
                EditView::new()
                    .content(
                        gethostname::gethostname()
                            .to_string_lossy()
                            .split('.')
                            .next()
                            .unwrap_or(""),
                    )
                    .on_submit({
                        let ui_tx = ui_tx.clone();
                        move |siv, username| {
                            ui_tx
                                .send(UICommand::UpdateUsername(username.to_string()))
                                .unwrap();
                            siv.pop_layer();
                            if init_after {
                                init_app(siv, ui_tx.clone());
                            }
                        }
                    })
                    .with_name("username_input"),
            )
            .button("Save", move |siv| {
                let username = siv
                    .call_on_name("username_input", |input: &mut EditView| input.get_content())
                    .unwrap();
                ui_tx
                    .send(UICommand::UpdateUsername(username.to_string()))
                    .unwrap();
                siv.pop_layer();
                if init_after {
                    init_app(siv, ui_tx.clone());
                }
            })
            .with_name("username_dialog")
            .full_width()
            .max_width(48),
    );
}

fn net_thread(tx: Sender<UICommand>, rx: Receiver<NetCommand>) {
    let id: Id = rand::thread_rng().gen();
    let mut username: String = "".to_string();
    let mut channel: Option<Channel> = None;
    let mut online: HashMap<Id, String> = HashMap::new();
    let mut is_join = true;

    loop {
        let res: Result<(), ArpchatError> = try {
            if channel.is_none() {
                if let Ok(NetCommand::SetInterface(name)) = rx.try_recv() {
                    let interface = sorted_usable_interfaces()
                        .into_iter()
                        .find(|iface| iface.name == name)
                        .ok_or(ArpchatError::InvalidInterface(name))?;
                    channel = Some(Channel::from_interface(interface)?);
                } else {
                    continue;
                }
            }
            // SAFETY: Checked directly above.
            let channel = unsafe { channel.as_mut().unwrap_unchecked() };

            match rx.try_recv() {
                Ok(NetCommand::SetInterface(_)) => Err(ArpchatError::InterfaceAlreadySet)?,
                Ok(NetCommand::SendMessage(msg)) => channel.send(Packet::Message(id, msg))?,
                Ok(NetCommand::UpdateUsername(new_username)) => {
                    username = new_username;
                    channel.send(Packet::PresenceReq)?;
                }
                Ok(NetCommand::Terminate) => {
                    let _ = channel.send(Packet::Disconnect(id));
                    break;
                }
                Err(_) => {}
            }

            match channel.try_recv()? {
                Some(Packet::Message(id, msg)) => {
                    let username = match online.get(&id) {
                        Some(username) => username.clone(),
                        None => "unknown".to_string(),
                    };
                    tx.send(UICommand::NewMessage(username, id, msg)).unwrap()
                }
                Some(Packet::PresenceReq) => {
                    channel.send(Packet::Presence(
                        id,
                        // First time we send a presence, we're joining.
                        if is_join {
                            is_join = false;
                            true
                        } else {
                            false
                        },
                        username.clone(),
                    ))?;
                }
                Some(Packet::Presence(id, is_join, new_username)) => {
                    match online.insert(id, new_username.clone()) {
                        Some(old_username) => tx
                            .send(UICommand::UpdatePresence(id, old_username, new_username))
                            .unwrap(),
                        None => tx
                            .send(UICommand::NewPresence(id, is_join, new_username))
                            .unwrap(),
                    }
                }
                Some(Packet::Disconnect(id)) => {
                    if let Some(username) = online.remove(&id) {
                        tx.send(UICommand::RemovePresence(id, username)).unwrap();
                    }
                }
                None => {}
            }
        };
        if let Err(err) = res {
            tx.send(UICommand::Error(err)).unwrap();
            break;
        }
    }
}

fn update_title(siv: &mut Cursive, username: &str, interface: &str) {
    let title = if interface.len() <= 8 {
        format!("arpchat: {username} ({interface})")
    } else {
        format!("arpchat: {username}")
    };
    siv.set_window_title(&title);
    siv.call_on_name(
        "chat_panel",
        |chat_panel: &mut Panel<ScrollView<ResizedView<ResizedView<NamedView<LinearLayout>>>>>| {
            chat_panel.set_title(title);
        },
    );
}

pub fn run() {
    let (mut username, mut interface) = ("anonymous".to_string(), "".to_string());

    let (ui_tx, ui_rx) = unbounded::<UICommand>();
    let (net_tx, net_rx) = unbounded::<NetCommand>();
    let net_thread = thread::spawn({
        let ui_tx = ui_tx.clone();
        move || net_thread(ui_tx, net_rx)
    });

    let mut siv = cursive::default();
    siv.load_toml(include_str!("../assets/theme.toml")).unwrap();

    show_iface_dialog(&mut siv, ui_tx);

    let mut siv = siv.runner();
    siv.refresh();
    while siv.is_running() {
        while let Ok(cmd) = ui_rx.try_recv() {
            match cmd {
                UICommand::NewMessage(username, id, msg) => {
                    siv.call_on_name("chat_inner", |chat_inner: &mut LinearLayout| {
                        chat_inner.add_child(
                            TextView::new(format!("[{username}] {msg}"))
                                .with_name(format!("{id:x?}_msg")),
                        );
                    });
                }
                UICommand::UpdateUsername(new_username) => {
                    if new_username == username {
                        continue;
                    }
                    if !new_username.is_empty() {
                        username = new_username;
                    }
                    net_tx
                        .send(NetCommand::UpdateUsername(username.clone()))
                        .unwrap();
                    update_title(&mut siv, &username, &interface);
                }
                UICommand::SetInterface(new_interface) => {
                    interface = new_interface;
                    net_tx
                        .send(NetCommand::SetInterface(interface.clone()))
                        .unwrap();
                    update_title(&mut siv, &username, &interface);
                }
                UICommand::SendMessage(msg) => {
                    if !msg.is_empty() {
                        net_tx.send(NetCommand::SendMessage(msg)).unwrap();
                    }
                }
                UICommand::NewPresence(id, is_join, username) => {
                    if is_join {
                        siv.call_on_name("chat_inner", |chat_inner: &mut LinearLayout| {
                            chat_inner.add_child(
                                TextView::new(
                                    format!("> {username} logged on").dark_grey().to_string(),
                                )
                                .with_name(format!("{id:x?}_logon")),
                            );
                        });
                    }
                    siv.call_on_name("presences", |presences: &mut LinearLayout| {
                        presences.add_child(
                            TextView::new(format!("* {username}"))
                                .with_name(format!("{id:x?}_presence")),
                        );
                    });
                }
                UICommand::UpdatePresence(id, old_username, new_username) => {
                    if old_username == new_username {
                        continue;
                    }
                    siv.call_on_name("chat_inner", |chat_inner: &mut LinearLayout| {
                        chat_inner.add_child(TextView::new(
                            format!("> {old_username} is now known as {new_username}")
                                .dark_grey()
                                .to_string(),
                        ));
                    });
                    siv.call_on_name(&format!("{id:x?}_logon"), |logon: &mut TextView| {
                        logon.set_content(
                            format!("> {new_username} logged on")
                                .dark_grey()
                                .to_string(),
                        );
                    });
                    siv.call_on_name(&format!("{id:x?}_presence"), |presence: &mut TextView| {
                        presence.set_content(format!("* {new_username}"));
                    });
                    siv.call_on_all_named(&format!("{id:x?}_msg"), |msg: &mut TextView| {
                        let body = msg
                            .get_content()
                            .source()
                            .split(' ')
                            .skip(1)
                            .collect::<Vec<&str>>()
                            .join(" ");
                        msg.set_content(format!("[{new_username}] {body}"));
                    });
                }
                UICommand::RemovePresence(id, username) => {
                    siv.call_on_name("chat_inner", |chat_inner: &mut LinearLayout| {
                        chat_inner.add_child(TextView::new(
                            format!("> {username} disconnected, baii~")
                                .dark_grey()
                                .to_string(),
                        ));
                    });
                    siv.call_on_name("presences", |presences: &mut LinearLayout| {
                        presences
                            .find_child_from_name(&format!("{id:x?}_presence"))
                            .map(|presence| presences.remove_child(presence));
                    });
                }
                UICommand::Error(err) => {
                    siv.add_layer(
                        Dialog::text(err.to_string())
                            .title("Error!")
                            .button("Exit", |siv| siv.quit()),
                    );
                    break;
                }
            }
            siv.refresh();
        }
        siv.step();
    }

    net_tx.send(NetCommand::Terminate).unwrap();
    net_thread.join().unwrap();
}
