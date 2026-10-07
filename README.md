# rsh

Tiny remote command runner through one public relay server.

```bash
# public server
export RSH_TOKEN='change-me'
rsh server

# each machine
export RSH_SERVER='server.example.com:7280'
export RSH_TOKEN='change-me'
rsh daemon --name mac

# anywhere
rsh ls
rsh mac "uname -a"
```

## Reconnect

`daemon` reconnects automatically after network loss with exponential backoff:

`1s -> 2s -> 4s -> ... -> 30s`

A successful connection resets the delay to 1 second. The server only keeps online nodes; a reconnected daemon simply registers again.

## Environment

- `RSH_TOKEN`: required shared token
- `RSH_SERVER`: server address, default `127.0.0.1:7280`
- `RSH_BIND`: server bind address, default `0.0.0.0:7280`

## Status

MVP only. Transport is plain TCP; the token and command payload are **not encrypted**. Do not expose it to an untrusted network yet. TLS/E2E encryption should be added before production use.
