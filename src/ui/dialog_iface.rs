use crossbeam_channel::Sender;
use cursive::traits::{Nameable, Resizable};
use cursive::views::{Dialog, SelectView};
use cursive::Cursive;

use crate::net::sorted_usable_interfaces;

use super::dialog_username::show_username_dialog;
use super::util::UICommand;

pub fn show_iface_dialog(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
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
