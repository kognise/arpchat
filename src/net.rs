use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::slice::Iter;

use pnet::datalink::{
    Channel as DataLinkChannel, DataLinkReceiver, DataLinkSender, NetworkInterface,
};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet as PnetPacket;
use pnet::util::MacAddr;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::error::ArpchatError;
use crate::ringbuffer::Ringbuffer;

const ARP_HTYPE: &[u8] = &[0x00, 0x01]; // Hardware Type (Ethernet)
const ARP_HLEN: u8 = 6; // Hardware Address Length
const ARP_OPER: &[u8] = &[0, 1]; // Operation (Request)
const PACKET_PREFIX: &[u8] = b"uwu";

pub const ID_SIZE: usize = 8;
pub const LEN_PREFIX_SIZE: usize = 8;
pub type Id = [u8; ID_SIZE];

// Tag, seq, and total, are each one byte, thus the `+ 3`.
const PACKET_PART_SIZE: usize = u8::MAX as usize - (PACKET_PREFIX.len() + 3 + ID_SIZE);

#[derive(Default, Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq)]
pub enum EtherType {
    #[default]
    Experimental1,
    Experimental2,
    IPv4,
}

impl EtherType {
    pub fn bytes(&self) -> &[u8] {
        match self {
            EtherType::Experimental1 => &[0x88, 0xb5],
            EtherType::Experimental2 => &[0x88, 0xb6],
            EtherType::IPv4 => &[0x08, 0x00],
        }
    }

    pub fn iter() -> Iter<'static, EtherType> {
        static TYPES: [EtherType; 3] = [
            EtherType::Experimental1,
            EtherType::Experimental2,
            EtherType::IPv4,
        ];
        TYPES.iter()
    }
}

impl Display for EtherType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EtherType::Experimental1 => write!(f, "experimental 1")?,
            EtherType::Experimental2 => write!(f, "experimental 2")?,
            EtherType::IPv4 => write!(f, "ipv4")?,
        }
        write!(
            f,
            " - 0x{:0>4x?}",
            u16::from_be_bytes(self.bytes().try_into().unwrap())
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Message {
        id: Id,
        author: Id,
        channel: String,
        message: String,
    },
    PresenceReq,
    Presence(Id, bool, String),
    Disconnect(Id),
    Reaction(Id, char),
}

impl Packet {
    fn tag(&self) -> u8 {
        match self {
            Packet::Message { .. } => 0,
            Packet::PresenceReq => 1,
            Packet::Presence(_, _, _) => 2,
            Packet::Disconnect(_) => 3,
            Packet::Reaction(_, _) => 4,
        }
    }

    fn deserialize(tag: u8, data: &[u8]) -> Option<Self> {
        match tag {
            0 => {
                let id_start = 0;
                let user_id_start = id_start + ID_SIZE;
                let chan_len_start = user_id_start + ID_SIZE;
                let chan_start = chan_len_start + LEN_PREFIX_SIZE;
                let chan_len =
                    u64::from_be_bytes(data[chan_len_start..chan_start].try_into().ok()?);
                let str_start = chan_start + chan_len as usize;

                let id: Id = data[id_start..user_id_start].try_into().ok()?;
                let user_id: Id = data[user_id_start..chan_len_start].try_into().ok()?;
                let chan = String::from_utf8(data[chan_start..str_start].to_vec()).ok()?;
                let raw_str = smaz::decompress(&data[str_start..]).ok()?;

                let str = String::from_utf8(raw_str).ok()?;

                Some(Packet::Message {
                    id,
                    author: user_id,
                    channel: chan,
                    message: str,
                })
            }
            1 => Some(Packet::PresenceReq),
            2 => {
                let id: Id = data[..ID_SIZE].try_into().ok()?;
                let is_join = data[ID_SIZE] > 0;
                let str = String::from_utf8(data[ID_SIZE + 1..].to_vec()).ok()?;
                Some(Packet::Presence(id, is_join, str))
            }
            3 => Some(Packet::Disconnect(data.try_into().ok()?)),
            4 => {
                let id: Id = data[..ID_SIZE].try_into().ok()?;
                let raw: [u8; 4] = data[ID_SIZE..].try_into().ok()?;

                Some(Packet::Reaction(
                    id,
                    char::from_u32(u32::from_be_bytes(raw))?,
                ))
            }
            _ => None,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        match self {
            Packet::Message {
                id,
                author,
                channel,
                message,
            } => [
                id as &[u8],
                author as &[u8],
                &(channel.len() as u64).to_be_bytes(),
                channel.as_bytes(),
                &smaz::compress(message.as_bytes()),
            ]
            .concat(),
            Packet::PresenceReq => vec![],
            Packet::Presence(id, is_join, str) => {
                [id as &[u8], &[*is_join as u8], str.as_bytes()].concat()
            }
            Packet::Disconnect(id) => id.to_vec(),
            Packet::Reaction(id, character) => {
                [id as &[u8], &u32::to_be_bytes(*character as u32)].concat()
            }
        }
    }
}

pub fn sorted_usable_interfaces() -> Vec<NetworkInterface> {
    let mut interfaces = pnet::datalink::interfaces()
        .into_iter()
        .filter(|iface| iface.mac.is_some() && !iface.ips.is_empty())
        .collect::<Vec<NetworkInterface>>();

    // Sort interfaces by descending number of IPs.
    interfaces.sort_unstable_by_key(|iface| iface.ips.len());
    interfaces.reverse();

    interfaces
}

pub struct Channel {
    src_mac: MacAddr,
    ether_type: EtherType,
    tx: Box<dyn DataLinkSender>,
    rx: Box<dyn DataLinkReceiver>,

    /// Buffer of received packet parts, keyed by the packet id.
    ///
    /// Each value is the Vec of its parts, and counts as a packet when
    /// every part is non-empty. There are probably several optimization
    /// opportunities here, but c'mon, a naive approach is perfectly fine
    /// for a program this cursed.
    buffer: HashMap<Id, Vec<Vec<u8>>>,

    /// Recent packet buffer for deduplication.
    recent: Ringbuffer<Id>,
}

impl Channel {
    pub fn from_interface(interface: NetworkInterface) -> Result<Self, ArpchatError> {
        let (tx, rx) = match pnet::datalink::channel(&interface, Default::default()) {
            Ok(DataLinkChannel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => return Err(ArpchatError::UnknownChannelType),
            Err(e) => return Err(ArpchatError::ChannelError(e)),
        };

        Ok(Self {
            src_mac: interface.mac.ok_or(ArpchatError::NoMAC)?,
            ether_type: EtherType::default(),
            tx,
            rx,
            buffer: HashMap::new(),
            recent: Ringbuffer::with_capacity(16),
        })
    }

    pub fn set_ether_type(&mut self, ether_type: EtherType) {
        self.ether_type = ether_type;
    }

    pub fn send(&mut self, packet: Packet) -> Result<(), ArpchatError> {
        let data = packet.serialize();
        let mut parts: Vec<&[u8]> = data.chunks(PACKET_PART_SIZE).collect();

        if parts.is_empty() {
            // We need to send some data so empty enums go through! Not entirely
            // sure *why* this is the case... pushing an empty string feels like
            // it should be fine, but it doesn't work.
            parts.push(b".");
        }
        if parts.len() - 1 > u8::MAX as usize {
            return Err(ArpchatError::MsgTooLong);
        }

        let total = (parts.len() - 1) as u8;
        let id: Id = rand::thread_rng().gen();
        for (seq, part) in parts.into_iter().enumerate() {
            self.send_part(packet.tag(), seq as u8, total, id, part)?;
        }

        Ok(())
    }

    fn send_part(
        &mut self,
        tag: u8,
        seq: u8,
        total: u8,
        id: Id,
        part: &[u8],
    ) -> Result<(), ArpchatError> {
        let data = &[PACKET_PREFIX, &[tag, seq, total], &id, part].concat();

        // The length of the data must fit in a u8. This should also
        // guarantee that we'll be inside the MTU.
        debug_assert!(
            data.len() <= u8::MAX as usize,
            "Part data is too large ({} > {})",
            data.len(),
            u8::MAX
        );

        let arp_buffer = [
            ARP_HTYPE,
            self.ether_type.bytes(),
            &[ARP_HLEN, data.len() as u8],
            ARP_OPER,
            &self.src_mac.octets(), // Sender hardware address
            data,                   // Sender protocol address
            &[0; 6],                // Target hardware address
            data,                   // Target protocol address
        ]
        .concat();

        let mut eth_buffer = vec![0; 14 + arp_buffer.len()];
        let mut eth_packet =
            MutableEthernetPacket::new(&mut eth_buffer).ok_or(ArpchatError::ARPSerializeFailed)?;
        eth_packet.set_destination(MacAddr::broadcast());
        eth_packet.set_source(self.src_mac);
        eth_packet.set_ethertype(EtherTypes::Arp);
        eth_packet.set_payload(&arp_buffer);

        match self.tx.send_to(eth_packet.packet(), None) {
            Some(Ok(())) => Ok(()),
            _ => Err(ArpchatError::ARPSendFailed),
        }
    }

    pub fn try_recv(&mut self) -> Result<Option<Packet>, ArpchatError> {
        let packet = self.rx.next().map_err(|_| ArpchatError::CaptureFailed)?;
        let packet = match EthernetPacket::new(packet) {
            Some(packet) => packet,
            None => return Ok(None),
        };

        // Early filter for packets that aren't relevant.
        if packet.get_ethertype() != EtherTypes::Arp
            || &packet.payload()[6..8] != ARP_OPER
            || &packet.payload()[..2] != ARP_HTYPE
            || packet.payload()[4] != ARP_HLEN
        {
            return Ok(None);
        }

        let data_len = packet.payload()[5] as usize;
        let data = &packet.payload()[14..14 + data_len];
        if !data.starts_with(PACKET_PREFIX) {
            return Ok(None);
        }

        if let &[tag, seq, total, ref inner @ ..] = &data[PACKET_PREFIX.len()..] {
            Ok(try {
                let id: Id = inner[..ID_SIZE].try_into().ok()?;
                let inner = &inner[ID_SIZE..];

                // Skip if we already have this packet.
                if self.recent.contains(&id) {
                    None?;
                }

                if let Some(parts) = self.buffer.get_mut(&id) {
                    parts[seq as usize] = inner.to_vec();
                } else {
                    let mut parts = vec![vec![]; total as usize + 1];
                    parts[seq as usize] = inner.to_vec();
                    self.buffer.insert(id, parts);
                }

                // SAFETY: Guaranteed to exist because it's populated directly above.
                let parts = unsafe { self.buffer.get(&id).unwrap_unchecked() };

                // Short-circuit if we don't have all the parts yet.
                if !parts.iter().all(|p| !p.is_empty()) {
                    None?;
                }

                // Put the packet together.
                let packet = Packet::deserialize(tag, &parts.concat());

                if packet.is_some() {
                    log::info!("received a {} packet", tag);
                    self.buffer.remove(&id);
                    self.recent.push(id);
                } else {
                    log::warn!("skipped a {} packet", tag);
                }

                packet?
            })
        } else {
            Ok(None)
        }
    }
}
