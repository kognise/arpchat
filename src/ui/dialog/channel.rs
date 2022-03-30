use crossbeam_channel::Sender;
use cursive::direction::Direction;
use cursive::traits::{Nameable, Resizable};
use cursive::views::{Dialog, EditView};
use cursive::{Cursive, View};

use crate::ui::util::UICommand;

pub fn show_channel_dialog(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
    if let Some(ref mut channel_dialog) = siv.find_name::<Dialog>("channel_dialog") {
        channel_dialog.take_focus(Direction::none()).unwrap();
        return;
    }

    siv.add_layer(
        Dialog::new()
            .title("set channel")
            .content(
                EditView::new()
                    .content("")
                    .on_submit({
                        let ui_tx = ui_tx.clone();
                        move |siv, channel| {
                            ui_tx
                                .send(UICommand::SetChannel(channel.to_string()))
                                .unwrap();
                            siv.pop_layer();
                        }
                    })
                    .with_name("channel_input"),
            )
            .button("Save", move |siv| {
                let channel = siv
                    .call_on_name("channel_input", |input: &mut EditView| input.get_content())
                    .unwrap();
                ui_tx
                    .send(UICommand::SetChannel(channel.to_string()))
                    .unwrap();
                siv.pop_layer();
            })
            .with_name("channel_dialog")
            .full_width()
            .max_width(48),
    );
}
