use crossbeam_channel::Sender;
use cursive::direction::Direction;
use cursive::traits::{Nameable, Resizable};
use cursive::views::{Dialog, LinearLayout, SelectView, TextView};
use cursive::{Cursive, View};

use crate::net::EtherType;

use crate::ui::config::CONFIG;
use crate::ui::util::UICommand;

pub fn show_ether_type_dialog(siv: &mut Cursive, ui_tx: Sender<UICommand>) {
    if let Some(ref mut ether_type_dialog) = siv.find_name::<Dialog>("ether_type_dialog") {
        ether_type_dialog.take_focus(Direction::none()).unwrap();
        return;
    }

    let preferred_index: Option<usize> = try {
        let ether_type = CONFIG.lock().ok()?.ether_type?;
        EtherType::iter().position(|et| et == &ether_type)?
    };

    siv.add_layer(
        Dialog::new()
            .title("switch protocol")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new(
                        "which protocol arpchat claims it's using.\n\nexperimental 1 and 2 are more standards-compliant and nicer to other devices, but ipv4 might be more reliable on some networks.\n ",
                    ))
                    .child(
                        SelectView::new()
                            .with_all(EtherType::iter().map(|et| (et.to_string(), et)))
                            .selected(preferred_index.unwrap_or_default())
                            .on_submit(move |siv, et: &EtherType| {
                                ui_tx.send(UICommand::SetEtherType(*et)).unwrap();
                                siv.pop_layer();
                            }),
                    ),
            )
            .with_name("ether_type_dialog")
            .full_width()
            .max_width(48),
    );
}
