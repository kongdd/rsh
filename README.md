# rsh

Tiny TLS-protected remote command runner through one public relay server.

## Usage

Create `~/.rsh/config.toml` interactively:

```bash
rsh config init
```

The command refuses to overwrite an existing file, sets mode `0600`, and generates server tokens automatically.

Server configuration:

```toml
cert = "/etc/letsencrypt/live/rsh.example.com/fullchain.pem"
key = "/etc/letsencrypt/live/rsh.example.com/privkey.pem"
daemon_token = "random-daemon-secret"
control_token = "different-control-secret"
nodes = "/var/lib/rsh/nodes.json"
```

```bash
rsh server
```

Node configuration:

```toml
server = "rsh.example.com:7280"
daemon_token = "random-daemon-secret"
```

```bash
rsh node add
rsh node add --name mac
```

Controller configuration:

```toml
server = "rsh.example.com:7280"
control_token = "different-control-secret"
```

```bash
rsh node list
rsh node list --json
rsh node status mac
rsh node rm mac
rsh run mac "uname -a"              # shell command
rsh run mac -- uname -a              # exact arguments
```

Install a systemd service with an absolute configuration path:

```bash
sudo RSH_CONFIG=/etc/rsh/config.toml rsh service install server
sudo RSH_CONFIG=/etc/rsh/config.toml rsh service install node --name mac
```

Environment variables still override the corresponding file values. The client verifies the server certificate against the Mozilla root store. For a private CA, set `ca`; if the dial address differs from the certificate name, set `server_name`.

## Safety

- TLS encrypts tokens, commands and output and verifies the server identity.
- Daemon and controller tokens are separate, limiting lateral movement if one daemon is compromised.
- Nodes are added on first connection and persisted as JSON on the server.
- `node list` reports every registered node, online state and last-seen time.
- Duplicate online device names are rejected instead of silently replaced.
- TLS/auth handshakes time out after 5 seconds.
- Commands are limited to 64 KiB; protocol messages to 8 MiB.
- Commands time out after 3600 seconds by default; override daemon-side `RSH_COMMAND_TIMEOUT`.
- The server waits 3660 seconds for a result; override `RSH_RESPONSE_TIMEOUT` when commands may run longer.
- Daemons reconnect with exponential backoff: `1s -> 2s -> 4s -> ... -> 30s`.
- Disconnected nodes remain registered and reconnect automatically.

There is intentionally no database, web UI, account system or P2P layer.
