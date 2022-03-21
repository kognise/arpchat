use std::collections::HashMap;

use pnet::datalink::{
    Channel as DataLinkChannel, DataLinkReceiver, DataLinkSender, NetworkInterface,
};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet;
use pnet::util::MacAddr;

use crate::error::ArpchatError;

const ARP_START: &[u8] = &[
    0, 1, // Hardware Type (Ethernet)
    8, 0, // Protocol Type (IPv4)
    6, // Hardware Address Length
];
const ARP_OPER: &[u8] = &[0, 1]; // Operation (Request)
const MSG_PREFIX: &[u8] = b"uwu";

// Seq is one byte and total is one byte, thus the `+ 2`.
const MSG_PART_SIZE: usize = u8::MAX as usize - (MSG_PREFIX.len() + 2);

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

    /// Buffer of received message parts, keyed by the number of parts.
    /// This keying method is far from foolproof but reduces the chance
    /// of colliding messages just a tiny bit.
    ///
    /// Each value is a Vec of its parts, and counts as a message when
    /// every part is non-empty. There are probably several optimization
    /// opportunities here, but c'mon, a naive approach is perfectly fine
    /// for a program this cursed.
    buffer: HashMap<u8, Vec<Vec<u8>>>,
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
            buffer: HashMap::new(),
        })
    }

    pub fn interface_name(&self) -> String {
        self.interface.name.clone()
    }

    pub fn send_msg(&mut self, msg: &str) -> Result<(), ArpchatError> {
        if msg.is_empty() {
            return Ok(());
        }

        let compressed = smaz::compress(msg.as_bytes());
        let parts: Vec<&[u8]> = compressed.chunks(MSG_PART_SIZE).collect();
        if parts.len() - 1 > u8::MAX as usize {
            return Err(ArpchatError::MsgTooLong);
        }

        let total = (parts.len() - 1) as u8;
        for (seq, part) in parts.into_iter().enumerate() {
            self.send_msg_part(seq as u8, total, part)?;
        }

        Ok(())
    }

    /// Send part of a message. Seq should be 0 for the **last** part,
    /// otherwise it should increase up to 255 and stay there.
    fn send_msg_part(&mut self, seq: u8, total: u8, data: &[u8]) -> Result<(), ArpchatError> {
        let data = [MSG_PREFIX, &[seq, total], data].concat();

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
            &data,                  // Sender protocol address
            &[0; 6],                // Target hardware address
            &data,                  // Target protocol address
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

    pub fn try_recv_msg(&mut self) -> Result<Option<String>, ArpchatError> {
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
        if &data[..MSG_PREFIX.len()] != MSG_PREFIX {
            return Ok(None);
        }

        if let [seq, total, ref compressed @ ..] = data[MSG_PREFIX.len()..] {
            if let Some(parts) = self.buffer.get_mut(&total) {
                parts[seq as usize] = compressed.to_vec();
            } else {
                let mut parts = vec![vec![]; total as usize + 1];
                parts[seq as usize] = compressed.to_vec();
                self.buffer.insert(total, parts);
            }

            // SAFETY: Guaranteed to be filled because it's populated directly above.
            let parts = unsafe { self.buffer.get(&total).unwrap_unchecked() };

            if parts.iter().all(|p| !p.is_empty()) {
                match smaz::decompress(&parts.concat()) {
                    Ok(raw_str) => {
                        self.buffer.remove(&total);
                        Ok(String::from_utf8(raw_str).ok())
                    }
                    Err(_) => Ok(None),
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}
