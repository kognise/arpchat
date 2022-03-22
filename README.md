# arpchat

so... you know arp? the protocol your computer uses to find the mac addresses of other computers on your network? yeah. that.

i thought it would be a great idea to hijack it to make a chat app :)

![screenshot of the tool in action](https://doggo.ninja/RJGHYH.png)

## motivation

1. once a year i'm on a network that i want to chat with friends over, but a captive portal blocks me
2. i'm completely insane
3. i'm a programmer

(i swear, i might actually briefly have a use for this! it might not be entirely useless! ... and other lies i tell myself)

## limitations

yes

## things i made arpchat do

you can send messages tens of thousands of characters long because i implemented a (naive) generalizable transport protocol on top of arp. there's also a bit of compression.

if you wanted, you could probably split off the networking part of this and use it instead of udp. please don't do this.

not only are join and leave notifications a thing, i built an entire presence discovery system to see a list of other online users. ironically, this serves a similar purpose to arp itself.

i'm planning on experimenting with and adding settings for different arp packet types, since some routers might filter out the malformed ip packets i'm currently using.

## running

if you actually want to install this for some reason, you can get it from [the releases page](https://github.com/kognise/arpchat/releases/latest).

on windows, you probably need [npcap](https://npcap.com/#download). make sure you check "Install Npcap in WinPcap API-compatible Mode" in the installer!

![interface selector](https://doggo.ninja/tvFJ2A.png)

then just run the binary in a terminal. you know it's working properly if you can see your own messages when you send them. if you *can't* see your messages, try selecting a different interface!

have any issues? that really sucks. you can make an issue if it pleases you.

![banner](https://doggo.ninja/fH9GKt.png)

## building

you don't really want to build this. anyway, it's tested on the latest unstable rust.

on windows, download the [WinPcap Developer's Pack](https://crates.io/crates/pnet#:~:text=WinPcap%20Developers%20pack) and set the `LIB` environment variable to the `WpdPack/Lib/x64/` folder.

```sh
cargo build
```
