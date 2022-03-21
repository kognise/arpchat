use pnet::datalink::{
    Channel as DataLinkChannel, DataLinkReceiver, DataLinkSender, NetworkInterface,
};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet;
use pnet::util::MacAddr;

const ARP_START: &[u8] = &[
    0, 1, // Hardware Type (Ethernet)
    8, 0, // Protocol Type (IPv4)
    6, // Hardware Address Length
];
const ARP_OPER: &[u8] = &[0, 1]; // Operation (Request)
const MSG_PREFIX: &[u8] = b"uwu";
pub const MAX_MSG_LEN: usize = u8::MAX as usize - MSG_PREFIX.len();

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
}

impl Channel {
    pub fn from_interface(interface: NetworkInterface) -> Self {
        let (tx, rx) = match pnet::datalink::channel(&interface, Default::default()) {
            Ok(DataLinkChannel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => panic!("Unknown channel type"),
            Err(e) => panic!("Error getting channel: {e}"),
        };

        Self {
            src_mac: interface.mac.unwrap(),
            interface,
            tx,
            rx,
        }
    }

    pub fn interface_name(&self) -> String {
        self.interface.name.clone()
    }

    pub fn send_msg(&mut self, msg: &str) {
        let data = [MSG_PREFIX, msg.as_bytes()].concat();

        // The length of data has to fit in a u8. This limitation should also
        // guarantee that we'll be inside the MTU.
        assert!(
            data.len() <= u8::MAX as usize,
            "Data is too large ({} > {})",
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
        let mut eth_packet = MutableEthernetPacket::new(&mut eth_buffer).unwrap();
        eth_packet.set_destination(MacAddr::broadcast());
        eth_packet.set_source(self.src_mac);
        eth_packet.set_ethertype(EtherTypes::Arp);
        eth_packet.set_payload(&arp_buffer);

        self.tx.send_to(eth_packet.packet(), None).unwrap().unwrap();
    }

    pub fn try_recv_msg(&mut self) -> Option<String> {
        let packet = self
            .rx
            .next()
            .unwrap_or_else(|e| panic!("Unable to capture packet: {e}"));
        let packet = EthernetPacket::new(packet).unwrap();

        // Early filter for packets that aren't relevant.
        if packet.get_ethertype() != EtherTypes::Arp
            || &packet.payload()[6..8] != ARP_OPER
            || &packet.payload()[0..5] != ARP_START
        {
            return None;
        }

        let data_len = packet.payload()[5] as usize;
        let data = &packet.payload()[14..14 + data_len];
        if &data[..MSG_PREFIX.len()] != MSG_PREFIX {
            return None;
        }

        String::from_utf8(data[MSG_PREFIX.len()..].to_vec()).ok()
    }
}
