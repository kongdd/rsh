# rsh

Tiny TLS-protected remote command runner through one public relay server.

## Usage

Server:

```bash
export RSH_CERT=/etc/letsencrypt/live/rsh.example.com/fullchain.pem
export RSH_KEY=/etc/letsencrypt/live/rsh.example.com/privkey.pem
export RSH_DAEMON_TOKEN='random-daemon-secret'
export RSH_CONTROL_TOKEN='different-control-secret'
rsh server
```

Each machine:

```bash
export RSH_SERVER='rsh.example.com:7280'
export RSH_DAEMON_TOKEN='random-daemon-secret'
rsh daemon                 # uses hostname
rsh daemon --name mac      # optional override
```

Controller:

```bash
export RSH_SERVER='rsh.example.com:7280'
export RSH_CONTROL_TOKEN='different-control-secret'
rsh ls
rsh mac "uname -a"
```

The client verifies the server certificate against the Mozilla root store. For a private CA, set `RSH_CA=/path/to/ca.pem`. If the dial address differs from the certificate name, set `RSH_SERVER_NAME=rsh.example.com`.

## Safety

- TLS encrypts tokens, commands and output and verifies the server identity.
- Daemon and controller tokens are separate, limiting lateral movement if one daemon is compromised.
- Duplicate online device names are rejected instead of silently replaced.
- TLS/auth handshakes time out after 5 seconds.
- Commands are limited to 64 KiB; protocol messages to 8 MiB.
- Commands time out after 3600 seconds by default; override with `RSH_COMMAND_TIMEOUT`.
- Daemons reconnect with exponential backoff: `1s -> 2s -> 4s -> ... -> 30s`.
- The server removes disconnected nodes; reconnecting daemons register again.

There is intentionally no database, web UI, account system or P2P layer.
