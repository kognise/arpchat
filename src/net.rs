use std::collections::HashMap;
use std::fmt::Debug;

use pnet::datalink::{
    Channel as DataLinkChannel, DataLinkReceiver, DataLinkSender, NetworkInterface,
};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet as PnetPacket;
use pnet::util::MacAddr;
use rand::Rng;

use crate::error::ArpchatError;

const ARP_START: &[u8] = &[
    0, 1, // Hardware Type (Ethernet)
    8, 0, // Protocol Type (IPv4)
    6, // Hardware Address Length
];
const ARP_OPER: &[u8] = &[0, 1]; // Operation (Request)
const PACKET_PREFIX: &[u8] = b"uwu";

// Tag, seq, and total, are each one byte, thus the `+ 3`.
const PACKET_PART_SIZE: usize = u8::MAX as usize - (PACKET_PREFIX.len() + 3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Message(String),
    PresenceReq,
    Presence,
    Disconnect,
}

impl Packet {
    fn tag(&self) -> u8 {
        match self {
            Packet::Message(_) => 0,
            Packet::PresenceReq => 1,
            Packet::Presence => 2,
            Packet::Disconnect => 3,
        }
    }

    fn deserialize(tag: u8, data: &[u8]) -> Option<Self> {
        match tag {
            0 => {
                let raw_str = smaz::decompress(data).ok()?;
                let str = String::from_utf8(raw_str).ok()?;
                Some(Packet::Message(str))
            }
            1 => Some(Packet::PresenceReq),
            2 => Some(Packet::Presence),
            3 => Some(Packet::Disconnect),
            _ => None,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        match self {
            Packet::Message(msg) => smaz::compress(msg.as_bytes()),
            Packet::PresenceReq => vec![],
            Packet::Presence => vec![],
            Packet::Disconnect => vec![],
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
    interface: NetworkInterface,
    src_mac: MacAddr,
    tx: Box<dyn DataLinkSender>,
    rx: Box<dyn DataLinkReceiver>,

    id: [u8; 8],
    online: HashMap<[u8; 8], String>,

    /// Buffer of received packet parts, keyed by the number of parts and
    /// its tag. This keying method is far from foolproof but reduces
    /// the chance of colliding packets just a tiny bit.
    ///
    /// Each value is the Vec of its parts, and counts as a packet when
    /// every part is non-empty. There are probably several optimization
    /// opportunities here, but c'mon, a naive approach is perfectly fine
    /// for a program this cursed.
    buffer: HashMap<(u8, u8), Vec<Vec<u8>>>,
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
            interface,
            tx,
            rx,
            id: rand::thread_rng().gen(),
            online: HashMap::new(),
            buffer: HashMap::new(),
        })
    }

    pub fn interface_name(&self) -> String {
        self.interface.name.clone()
    }

    pub fn send(&mut self, packet: Packet) -> Result<(), ArpchatError> {
        let data = packet.serialize();
        let parts: Vec<&[u8]> = data.chunks(PACKET_PART_SIZE).collect();
        if parts.len() - 1 > u8::MAX as usize {
            return Err(ArpchatError::MsgTooLong);
        }

        let total = (parts.len() - 1) as u8;
        for (seq, part) in parts.into_iter().enumerate() {
            self.send_part(packet.tag(), seq as u8, total, part)?;
        }

        Ok(())
    }

    fn send_part(&mut self, tag: u8, seq: u8, total: u8, part: &[u8]) -> Result<(), ArpchatError> {
        let data = &[PACKET_PREFIX, &[tag, seq, total], part].concat();

        // The length of the data must fit in a u8. This should also
        // guarantee that we'll be inside the MTU.
        debug_assert!(
            data.len() <= u8::MAX as usize,
            "Part data is too large ({} > {})",
            data.len(),
            u8::MAX
        );

        let arp_buffer = [
            ARP_START,
            &[data.len() as u8], // Protocol Address Length
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
            || &packet.payload()[..5] != ARP_START
        {
            return Ok(None);
        }

        let data_len = packet.payload()[5] as usize;
        let data = &packet.payload()[14..14 + data_len];
        if !data.starts_with(PACKET_PREFIX) {
            return Ok(None);
        }

        if let &[tag, seq, total, ref inner @ ..] = &data[PACKET_PREFIX.len()..] {
            let key = (tag, total);

            if let Some(parts) = self.buffer.get_mut(&key) {
                parts[seq as usize] = inner.to_vec();
            } else {
                let mut parts = vec![vec![]; total as usize + 1];
                parts[seq as usize] = inner.to_vec();
                self.buffer.insert(key, parts);
            }

            // SAFETY: Guaranteed to exist because it's populated directly above.
            let parts = unsafe { self.buffer.get(&key).unwrap_unchecked() };

            if parts.iter().all(|p| !p.is_empty()) {
                let packet = Packet::deserialize(tag, &parts.concat());
                if packet.is_some() {
                    self.buffer.remove(&key);
                }
                Ok(packet)
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}
