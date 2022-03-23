use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use rand::Rng;

use crate::error::ArpchatError;
use crate::net::{sorted_usable_interfaces, Channel, Id, Packet};

use super::config::CONFIG;
use super::util::UpdatePresenceKind;
use super::{NetCommand, UICommand};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const INACTIVE_TIMEOUT: Duration = Duration::from_secs(5);
const OFFLINE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum NetThreadState {
    NeedsUsername,
    NeedsInitialPresence,
    Ready,
}

pub(super) fn start_net_thread(tx: Sender<UICommand>, rx: Receiver<NetCommand>) {
    let id: Id = rand::thread_rng().gen();
    let mut username: String = "".to_string();
    let mut channel: Option<Channel> = None;

    let mut last_heartbeat = Instant::now();
    let mut online: HashMap<Id, (Instant, String)> = HashMap::new();
    let mut offline: HashSet<Id> = HashSet::new();

    let mut state = NetThreadState::NeedsUsername;
    let mut pause_heartbeat = false;

    loop {
        let res: Result<(), ArpchatError> = try {
            if channel.is_none() {
                if let Ok(NetCommand::SetInterface(name)) = rx.try_recv() {
                    let interface = sorted_usable_interfaces()
                        .into_iter()
                        .find(|iface| iface.name == name)
                        .ok_or(ArpchatError::InvalidInterface(name))?;

                    let mut new_channel = Channel::from_interface(interface)?;
                    if let Some(ether_type) = CONFIG.lock().unwrap().ether_type {
                        new_channel.set_ether_type(ether_type);
                    }
                    channel = Some(new_channel);
                } else {
                    continue;
                }
            }
            // SAFETY: Checked directly above.
            let channel = unsafe { channel.as_mut().unwrap_unchecked() };

            match rx.try_recv() {
                Ok(NetCommand::SetInterface(_)) => Err(ArpchatError::InterfaceAlreadySet)?,
                Ok(NetCommand::SetEtherType(ether_type)) => channel.set_ether_type(ether_type),
                Ok(NetCommand::SendMessage(chan, msg)) => {
                    channel.send(Packet::Message(id, chan, msg))?
                }
                Ok(NetCommand::UpdateUsername(new_username)) => {
                    username = new_username;
                    if state == NetThreadState::NeedsUsername {
                        channel.send(Packet::PresenceReq)?;
                        state = NetThreadState::NeedsInitialPresence;
                    }
                }
                Ok(NetCommand::Terminate) => {
                    let _ = channel.send(Packet::Disconnect(id));
                    break;
                }
                Ok(NetCommand::PauseHeartbeat(pause)) => pause_heartbeat = pause,
                Err(_) => {}
            }

            match channel.try_recv()? {
                Some(Packet::Message(id, channel, message)) => {
                    let username = match online.get(&id) {
                        Some((_, username)) => username.clone(),
                        None => "unknown".to_string(),
                    };
                    tx.send(UICommand::NewMessage {
                        username,
                        channel,
                        message,
                    })
                    .unwrap()
                }
                Some(Packet::PresenceReq) => {
                    if state == NetThreadState::NeedsInitialPresence {
                        channel.send(Packet::Presence(id, true, username.clone()))?;
                    } else {
                        channel.send(Packet::Presence(id, false, username.clone()))?;
                    }
                }
                Some(Packet::Presence(pres_id, is_join, username)) => {
                    match online.insert(pres_id, (Instant::now(), username.clone())) {
                        Some((_, former)) => {
                            tx.send(UICommand::PresenceUpdate(
                                pres_id,
                                username,
                                false,
                                UpdatePresenceKind::UsernameChange(former),
                            ))
                            .unwrap();
                        }
                        None => {
                            tx.send(UICommand::PresenceUpdate(
                                pres_id,
                                username,
                                false,
                                if offline.remove(&id) || is_join {
                                    UpdatePresenceKind::JoinOrReconnect
                                } else {
                                    UpdatePresenceKind::Boring
                                },
                            ))
                            .unwrap();
                        }
                    }

                    if pres_id == id {
                        state = NetThreadState::Ready;
                    }
                }
                Some(Packet::Disconnect(id)) => {
                    if let Some((_, username)) = online.remove(&id) {
                        tx.send(UICommand::RemovePresence(id, username)).unwrap();
                    }
                }
                None => {}
            }

            if last_heartbeat.elapsed() > HEARTBEAT_INTERVAL && state == NetThreadState::Ready {
                if !pause_heartbeat {
                    channel.send(Packet::Presence(id, false, username.clone()))?;
                }

                let mut to_remove = vec![];
                for (id, (last_heartbeat, username)) in online.iter() {
                    if last_heartbeat.elapsed() > OFFLINE_TIMEOUT {
                        offline.insert(*id);
                        tx.send(UICommand::RemovePresence(*id, username.clone()))
                            .unwrap();
                        to_remove.push(*id);
                    } else if last_heartbeat.elapsed() > INACTIVE_TIMEOUT {
                        tx.send(UICommand::PresenceUpdate(
                            *id,
                            username.clone(),
                            true,
                            UpdatePresenceKind::Boring,
                        ))
                        .unwrap();
                    }
                }
                for id in to_remove {
                    online.remove(&id);
                }

                last_heartbeat = Instant::now();
            }
        };
        if let Err(err) = res {
            tx.send(UICommand::Error(err)).unwrap();
            break;
        }
    }
}
