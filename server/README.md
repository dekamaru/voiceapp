# voiceapp-server

Voice application server with TCP management and UDP voice relay.

## Architecture

The server runs two concurrent components:

- **ManagementServer** (TCP) - User authentication, presence, chat, mute state sync
- **VoiceRelayServer** (UDP) - Token-based voice authentication and voice data forwarding

Communication between servers is handled via an async event channel.

## Sequence Diagram

```
┌────────┐          ┌──────────────────┐          ┌────────────────┐
│ Client │          │ ManagementServer │          │VoiceRelayServer│
└───┬────┘          └───────┬──────────┘          └───────┬────────┘
    │                       │                             │
    │  TCP: LoginRequest    │                             │
    │──────────────────────>│                             │
    │                       │  Event: UserConnected       │
    │                       │  (id, token)                │
    │                       │────────────────────────────>│
    │  LoginResponse        │                             │
    │  (id, token, users)   │                             │
    │<──────────────────────│                             │
    │                       │                             │
    │  UDP: VoiceAuthReq ────────────────────────────────>│
    │  (token)              │                             │
    │  UDP: VoiceAuthResp <───────────────────────────────│
    │                       │                             │
    │  TCP: JoinVoiceReq    │                             │
    │──────────────────────>│                             │
    │                       │  Event: VoiceJoined         │
    │                       │────────────────────────────>│
    │  JoinVoiceResponse    │                             │
    │<──────────────────────│                             │
    │                       │                             │
    │  UDP: VoiceData ───────────────────────────────────>│
    │                       │                  ┌──────────┴─────────┐
    │                       │                  │  Relay to others   │
    │                       │                  │  in voice channel  │
    │                       │                  └──────────┬─────────┘
    │  UDP: VoiceData <───────────────────────────────────│
    │                       │                             │
```

## Authentication Flow

1. **TCP Connection** - Client connects to ManagementServer, server assigns user ID and generates random token
2. **Login** - Client sends `LoginRequest` with username, server responds with user ID, voice token, and participant list
3. **UDP Auth** - Client sends `VoiceAuthRequest` with token to VoiceRelayServer
4. **Validation** - Server validates token, associates UDP address with user, responds with success/failure
5. **Voice Ready** - Client can now join voice channel and send/receive voice packets

The token-based approach allows the UDP server to verify that voice packets come from authenticated TCP sessions without sharing state directly.

## Usage

### As Binary

```bash
cargo run --release -p voiceapp-server
```

### As Library

```rust
use voiceapp_server::{ManagementServer, VoiceRelayServer};

#[tokio::main]
async fn main() {
    let (mgmt_server, events_rx) = ManagementServer::new();
    let mut voice_server = VoiceRelayServer::new(events_rx);

    tokio::spawn(async move { mgmt_server.run(9001).await });
    voice_server.run(9002).await;
}
```

## Configuration

Environment variables with defaults:

| Variable | Default | Description |
|----------|---------|-------------|
| `MANAGEMENT_PORT` | 9001 | TCP server port |
| `VOICE_RELAY_PORT` | 9002 | UDP server port |

## Protocol

Uses `voiceapp-protocol` for packet encoding/decoding. See protocol crate for packet types.

## License

MIT
