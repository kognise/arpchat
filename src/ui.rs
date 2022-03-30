// This is all horrible and needs a humongous refactor.
// The net code is half-decent though!

mod config;
mod init;
mod net_thread;
mod util;

mod dialog {
    pub mod channel;
    pub mod ether_type;
    pub mod interface;
    pub mod username;
}

use std::thread;

use crossbeam_channel::unbounded;
use cursive::backends::crossterm::crossterm::style::Stylize;
use cursive::traits::{Nameable, Resizable};
use cursive::views::{Dialog, EditView, LinearLayout, TextView};

use self::config::CONFIG;
use self::dialog::interface::show_iface_dialog;
use self::util::{
    append_txt, clear_children, update_or_append_txt, update_title, NetCommand, UICommand,
    UpdatePresenceKind,
};

pub fn run() {
    let (mut username, mut interface, mut current_channel) =
        ("anonymous".to_string(), "".to_string(), "".to_string());

    let (ui_tx, ui_rx) = unbounded::<UICommand>();
    let (net_tx, net_rx) = unbounded::<NetCommand>();
    let net_thread = thread::spawn({
        let ui_tx = ui_tx.clone();
        move || net_thread::start_net_thread(ui_tx, net_rx)
    });

    let mut siv = cursive::default();
    siv.load_toml(include_str!("../assets/theme.toml")).unwrap();

    show_iface_dialog(&mut siv, ui_tx);

    let mut siv = siv.runner();
    siv.refresh();
    while siv.is_running() {
        while let Ok(cmd) = ui_rx.try_recv() {
            match cmd {
                UICommand::NewMessage {
                    id,
                    username,
                    channel,
                    message,
                } => {
                    if channel == current_channel {
                        let net_tx = net_tx.clone();
                        siv.call_on_name("chat_inner", move |parent: &mut LinearLayout| {
                            parent.add_child(
                                LinearLayout::horizontal()
                                    .child(
                                        TextView::new(format!("[{username}] {message}"))
                                            .full_width(),
                                    )
                                    .child(
                                        EditView::new()
                                            .max_content_width(3)
                                            .on_submit(move |siv, reaction| {
                                                if let Some(character) = reaction.chars().next() {
                                                    siv.call_on_name(
                                                        &format!("{id}_reaction_box"),
                                                        |input: &mut EditView| {
                                                            input.set_content("");
                                                        },
                                                    );
                                                    net_tx
                                                        .send(NetCommand::Reaction(id, character))
                                                        .unwrap();
                                                }
                                            })
                                            .with_name(format!("{id}_reaction_box"))
                                            .min_width(3),
                                    )
                                    .with_name(format!("{id}_message")),
                            );
                        });
                    }
                }
                UICommand::UpdateUsername(new_username) => {
                    if new_username == username {
                        continue;
                    }
                    if !new_username.is_empty() {
                        username = new_username;

                        let mut config = CONFIG.lock().unwrap();
                        config.username = Some(username.clone());
                        config.save();
                    }

                    net_tx
                        .send(NetCommand::UpdateUsername(username.clone()))
                        .unwrap();
                    update_title(&mut siv, &username, &interface, &current_channel);
                }
                UICommand::SetInterface(new_interface) => {
                    interface = new_interface;
                    net_tx
                        .send(NetCommand::SetInterface(interface.clone()))
                        .unwrap();
                    update_title(&mut siv, &username, &interface, &current_channel);

                    let mut config = CONFIG.lock().unwrap();
                    config.interface = Some(interface.clone());
                    config.save();
                }
                UICommand::SetEtherType(ether_type) => {
                    net_tx.send(NetCommand::SetEtherType(ether_type)).unwrap();

                    let mut config = CONFIG.lock().unwrap();
                    config.ether_type = Some(ether_type);
                    config.save();
                }
                UICommand::SendMessage(msg) => {
                    if msg == "/offline" {
                        net_tx.send(NetCommand::PauseHeartbeat(true)).unwrap();
                    } else if msg == "/online" {
                        net_tx.send(NetCommand::PauseHeartbeat(false)).unwrap();
                    } else if !msg.is_empty() {
                        net_tx
                            .send(NetCommand::SendMessage(current_channel.clone(), msg))
                            .unwrap();
                    }
                }
                UICommand::PresenceUpdate(id, username, is_inactive, kind) => {
                    match kind {
                        UpdatePresenceKind::JoinOrReconnect => {
                            append_txt(
                                &mut siv,
                                "chat_inner",
                                format!("> {username} logged on").dark_grey().to_string(),
                            );
                        }
                        UpdatePresenceKind::UsernameChange(former) if former != username => {
                            append_txt(
                                &mut siv,
                                "chat_inner",
                                format!("> {former} is now known as {username}")
                                    .dark_grey()
                                    .to_string(),
                            );
                        }
                        _ => {}
                    }

                    // Update username in presences list.
                    update_or_append_txt(
                        &mut siv,
                        "presences",
                        &format!("{id:x?}_presence"),
                        match is_inactive {
                            true => format!("- {username}").dark_grey().to_string(),
                            false => format!("* {username}"),
                        },
                    );
                }
                UICommand::RemovePresence(id, username) => {
                    append_txt(
                        &mut siv,
                        "chat_inner",
                        format!("> {username} disconnected, baii~")
                            .dark_grey()
                            .to_string(),
                    );

                    // Remove from presences list.
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
                UICommand::SetChannel(channel) => {
                    current_channel = channel;
                    clear_children(&mut siv, "chat_inner");
                    update_title(&mut siv, &username, &interface, &current_channel);
                }
                UICommand::Reaction(id, character) => {
                    siv.call_on_name(&format!("{id}_message"), |view: &mut LinearLayout| {
                        view.add_child(TextView::new(character));
                    });
                }
            }
            siv.refresh();
        }
        siv.step();
    }

    net_tx.send(NetCommand::Terminate).unwrap();
    net_thread.join().unwrap();
    CONFIG.lock().unwrap().save();
}
