use crossbeam_channel::Sender;
use cursive::event::Key;
use cursive::traits::{Nameable, Resizable, Scrollable};
use cursive::view::ScrollStrategy;
use cursive::views::{EditView, LinearLayout, Panel};
use cursive::Cursive;

use super::dialog::channel::show_channel_dialog;
use super::dialog::ether_type::show_ether_type_dialog;
use super::dialog::username::show_username_dialog;
use super::util::UICommand;

pub fn init_app(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
    siv.menubar()
        .add_leaf("set username", {
            let ui_tx = ui_tx.clone();
            move |siv| show_username_dialog(siv, ui_tx.clone(), false)
        })
        .add_leaf("switch protocol", {
            let ui_tx = ui_tx.clone();
            move |siv| show_ether_type_dialog(siv, ui_tx.clone())
        })
        .add_leaf("set channel", {
            let ui_tx = ui_tx.clone();
            move |siv| show_channel_dialog(siv, ui_tx.clone())
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
