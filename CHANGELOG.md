# Changelog

## 0.1.1 — 2026-11-22

### Measurement

- Each resolver now gets 5 cached lookups instead of one, reported as median, p95, jitter and loss. The fastest is picked by median among servers with at most 20% loss, with p95 as the tiebreak.
- New **Cold** column: one lookup of a random, never-cached name, so the resolver has to ask the authoritative servers. This is the delay you feel on sites you visit for the first time.
- New **Site** column: which anycast location answered, from a CHAOS TXT `id.server` query (Cloudflare, Quad9, Level 3 and others report it).
- Replies are now checked for the matching transaction id and the response flag, so a stray datagram can't be counted as an answer.
- Servers are measured 6 at a time instead of one after another. Probing every server at once caused false packet loss on home routers.
- In simple mode, **Find fastest** now tests the five servers on screen and shows each one's time next to it.

### App

- The window shows which DNS the selected adapter is using right now.
- Release builds ask for administrator rights at launch, so **Apply** no longer fails with *Permission denied*.
- Larger window so the new columns fit.
- Removed Norton ConnectSafe (all three addresses no longer reply).

## 0.1.0 — 2026-11-21

First public release.
