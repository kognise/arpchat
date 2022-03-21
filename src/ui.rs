use std::thread;

use crossbeam_channel::{unbounded, Receiver, Sender};
use cursive::direction::Direction;
use cursive::event::Key;
use cursive::traits::{Nameable, Resizable, Scrollable};
use cursive::view::ScrollStrategy;
use cursive::views::{
    Dialog, EditView, LinearLayout, NamedView, Panel, ResizedView, ScrollView, SelectView, TextView,
};
use cursive::{Cursive, View};

use crate::error::ArpchatError;
use crate::net::{sorted_usable_interfaces, Channel, Packet};

enum UICommand {
    NewMessage(String),
    SendNickedMessage(String),
    UpdateUsername(String),
    SwitchInterface(String),
    Error(ArpchatError),
}

enum NetCommand {
    SendMessage(String),
    SwitchInterface(String),
    Terminate,
}

fn init_app(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
    siv.menubar()
        .add_leaf("Set Username", {
            let ui_tx = ui_tx.clone();
            move |siv| show_username_dialog(siv, ui_tx.clone(), false)
        })
        .add_leaf("Quit", |siv| siv.quit());
    siv.set_autohide_menu(false);
    siv.add_global_callback(Key::Esc, |siv| siv.select_menubar());

    siv.add_fullscreen_layer(
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
                            ui_tx
                                .send(UICommand::SendNickedMessage(msg.to_string()))
                                .unwrap();
                        })
                        .with_name("input"),
                )
                .full_width(),
            ),
    );
}

fn show_iface_dialog(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
    siv.add_layer(
        Dialog::new()
            .title("Select an Interface")
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
                        ui_tx
                            .send(UICommand::SwitchInterface(name.clone()))
                            .unwrap();
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
            .title("Set Username")
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
            .min_width(48),
    );
}

fn net_thread(tx: Sender<UICommand>, rx: Receiver<NetCommand>) {
    let mut channel: Option<Channel> = None;

    'outer: loop {
        let res: Result<(), ArpchatError> = try {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    NetCommand::SendMessage(msg) => {
                        if let Some(ref mut channel) = channel {
                            channel.send(Packet::Message(msg))?;
                        }
                    }
                    NetCommand::SwitchInterface(name) => {
                        if let Some(ref channel) = channel && channel.interface_name() == name {
                        continue;
                    }
                        let interface = sorted_usable_interfaces()
                            .into_iter()
                            .find(|iface| iface.name == name)
                            .ok_or(ArpchatError::InvalidInterface(name))?;
                        channel = Some(Channel::from_interface(interface)?);
                    }
                    NetCommand::Terminate => {
                        break 'outer;
                    }
                }
            }

            if let Some(ref mut channel) = channel && let Some(Packet::Message(msg)) = channel.try_recv()? {
                tx.send(UICommand::NewMessage(msg)).unwrap();
            }
        };
        if let Err(err) = res {
            tx.send(UICommand::Error(err)).unwrap();
            break;
        }
    }
}

fn update_title(siv: &mut Cursive, username: &str, interface: &str) {
    let title = &format!("arpchat: {} ({})", username, interface);
    siv.set_window_title(title);
    siv.call_on_name(
        "chat_panel",
        |chat_panel: &mut Panel<ScrollView<ResizedView<ResizedView<NamedView<LinearLayout>>>>>| {
            chat_panel.set_title(title);
        },
    );
}

pub fn run() {
    let (mut username, mut interface) = ("anonymous".to_string(), "".to_string());
    let mut is_first_username = true;

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
                UICommand::NewMessage(msg) => {
                    siv.call_on_name("chat_inner", |chat_inner: &mut LinearLayout| {
                        chat_inner.add_child(TextView::new(msg));
                    });
                }
                UICommand::UpdateUsername(new_username) => {
                    if !new_username.is_empty() {
                        username = new_username;
                    }
                    update_title(&mut siv, &username, &interface);
                    if is_first_username {
                        net_tx
                            .send(NetCommand::SendMessage(format!("> {username} logged on")))
                            .unwrap();
                        is_first_username = false;
                    }
                }
                UICommand::SwitchInterface(new_interface) => {
                    net_tx
                        .send(NetCommand::SwitchInterface(new_interface.clone()))
                        .unwrap();
                    interface = new_interface;
                    update_title(&mut siv, &username, &interface);
                }
                UICommand::SendNickedMessage(msg) => {
                    if !msg.is_empty() {
                        net_tx
                            .send(NetCommand::SendMessage(format!("[{username}] {msg}")))
                            .unwrap();
                    }
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

    net_tx
        .send(NetCommand::SendMessage(format!(
            "> {username} disconnected"
        )))
        .unwrap();
    net_tx.send(NetCommand::Terminate).unwrap();
    net_thread.join().unwrap();
}
