<p align="center">
  <img src="assets/logo.svg" alt="" width="180">
</p>

<h1 align="center">DNS Switcher</h1>

<p align="center">
  Change your Windows DNS in one click and find out which resolver answers fastest from your network.
</p>

<p align="center">
  <a href="https://github.com/Devzinh/dns-switcher/releases">Download</a> &nbsp;&nbsp; <a href="#how-it-works">How it works</a> &nbsp;&nbsp; <a href="#roadmap">Roadmap</a>
</p>

<p align="center">
  <b>English</b> &nbsp;|&nbsp; <a href="README.pt-BR.md">Português</a>
</p>

<br>

A single executable for Windows 10 and 11. No installer, no telemetry, nothing left running in the background.

<p align="center">
  <img src="assets/query.en.svg" alt="DNS query in hex and the reply time of each resolver" width="100%">
</p>

Above is the exact packet the **Find fastest** button sends. The same packet goes to every resolver, and the first one to reply gets selected.

## What it does

<img src="assets/screenshot.png" alt="DNS Switcher window in simple mode" width="380" align="right">

**Simple mode.** Five well-known resolvers, each with one line saying what it's for. Pick one and click **Apply DNS**. The **Automatic** button gives you back your provider's DNS.

**Advanced mode.** More than 30 resolvers in three groups: general use, malware blocking and family filtering. It also has IPv6, a custom server, network adapter selection and the reply time of each server.

**Find fastest** measures every server in the group and selects the fastest. **Flush DNS** clears the Windows cache, so the change takes effect right away.

<br clear="right">

## How it works

The app only uses tools that ship with Windows, plus a UDP socket.

| Step | What happens |
|---|---|
| List adapters | Reads `netsh interface ipv4 show interfaces` by column, so it works with Windows in any language. Skips loopback. |
| Measure | Builds an A-type query for `www.google.com` by hand (RFC 1035) and sends it over UDP to port 53. The clock runs until the first valid reply arrives. The limit is 500 ms. |
| Apply | `netsh interface ipv4 set dnsservers … static` for the primary server and `add dnsservers … index=2` for the secondary. For IPv6 it's the same command in the `ipv6` family. |
| Restore | Switches IPv4 and IPv6 back to DHCP. |
| Clear cache | `ipconfig /flushdns`. |

Everything that touches the network or the system runs off the UI thread, which gets the result back through a channel. The window never freezes. The app doesn't write files or registry keys either: Windows itself keeps the configuration.

The current measurement sends **one query per server**. That's enough to compare resolvers against each other, but it doesn't show variation (jitter), packet loss or the worst case. That's on the roadmap.

```
src/
  main.rs       UI (egui), simple and advanced modes
  providers.rs  IPv4 and IPv6 resolver list, by group
  sys.rs        netsh, ipconfig, adapters and the UDP query
```

## Installation

Download `dns_switcher.exe` from [Releases](https://github.com/Devzinh/dns-switcher/releases), right-click it and choose **Run as administrator**.

Changing DNS requires administrator rights. Without them, measuring and clearing the cache still work, but **Apply** and **Automatic** show *Permission denied*.

The executable isn't digitally signed yet. If SmartScreen warns you, click **More info** and then **Run anyway**. You can also build it yourself:

```powershell
git clone https://github.com/Devzinh/dns-switcher.git
cd dns-switcher
cargo build --release
.\target\release\dns_switcher.exe
```

Building requires stable Rust ([rustup.rs](https://rustup.rs)).

## Included resolvers

| Group | Purpose | Examples |
|---|---|---|
| Default | General use | Cloudflare, Google, Quad9, OpenDNS, AdGuard, NextDNS, DNS4EU |
| Secure | Blocks malware and phishing | Cloudflare 1.1.1.2, CleanBrowsing Secure, DNS4EU Protective |
| Family Safe | Blocks adult content | Cloudflare 1.1.1.3, OpenDNS FamilyShield, AdGuard Family |

The full list is in [`src/providers.rs`](src/providers.rs). Some older services on that list, such as Norton ConnectSafe, Verisign and Dyn, were shut down by their operators and no longer reply. The speed test simply leaves them out.

## Roadmap

### AI-powered Fastest DNS

Today, **Find fastest** times a fixed list. The goal is for it to answer the question that matters: *will this DNS make my connection better, from here, right now?*

- [ ] You use your own API key (Claude, OpenAI and others). It's stored in Windows Credential Manager and never passes through any project server.
- [ ] Searches the web for this year's resolvers: new services, changed IPs, shut-down services and changes to data-logging policies. The built-in list becomes just a starting point.
- [ ] Locates your connection: public IP, carrier (ASN) and region.
- [ ] Finds out which of the resolver's servers is actually answering. The same IP exists in many countries (anycast), and a `CHAOS TXT id.server` or NSID (RFC 5001) query reveals which one replied.
- [ ] Checks whether the resolver passes part of your IP on to CDNs (EDNS Client Subnet, RFC 7871). That decides which YouTube or Netflix server you get sent to.
- [ ] Measures each server several times and computes median, p95, variation and loss. Also separates cached replies from full lookups, using a random domain.
- [ ] Produces a score with an explanation: plain language for beginners, and the numbers for anyone who wants to check.
- [ ] Keeps working exactly as today for anyone who doesn't provide a key.

### Network and security

- [ ] Encrypted DNS (DoH and DoT), using Windows 11's native support
- [ ] Check whether the resolver validates DNSSEC
- [ ] Detect when the ISP intercepts queries or redirects nonexistent domains
- [ ] Detect DNS leaks while a VPN is on

### Everyday use

- [ ] Tray icon with quick switching
- [ ] Per-network profiles (home, work, public Wi-Fi)
- [ ] Ask for administrator rights on launch and sign the executable
- [ ] Interface in Portuguese, English and Spanish
- [ ] Command line for scripts: `dns_switcher --apply cloudflare --adapter "Wi-Fi"`

## FAQ

<details>
<summary>Does changing DNS increase download speed?</summary>
<br>
Not directly. DNS only translates names into addresses. The gain is in the time to the first connection with each site. Also, depending on the resolver, the CDN may send you to a closer server, and that affects streaming and gaming.
</details>

<details>
<summary>Why isn't the fastest one for me the fastest for someone else?</summary>
<br>
Public resolvers use anycast: the same IP exists in many places around the world. Which one serves you depends on the route your carrier picks, not on distance on a map. That's why the measurement has to run on your connection.
</details>

<details>
<summary>Does the app collect any data?</summary>
<br>
No. The only thing it sends over the network is the test queries to the resolvers you're measuring.
</details>

<details>
<summary>How do I undo it?</summary>
<br>
Click <b>Automatic</b> in simple mode or <b>Default</b> in advanced mode. That's equivalent to <code>netsh interface ipv4 set dnsservers name="Wi-Fi" dhcp</code>, plus the same command for IPv6.
</details>

## Contributing

To add or fix a resolver, edit `src/providers.rs` and include a link to the service's official page in the pull request. Run `cargo fmt` and `cargo clippy` before opening the PR.

When reporting a bug, include your Windows version and language, and paste the output of `netsh interface ipv4 show interfaces`.

## License

[MIT](LICENSE)
