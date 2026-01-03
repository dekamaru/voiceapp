# voiceapp-protocol

Binary protocol for voice application client-server communication.

## Wire Format

All packets follow the same structure:

```
[packet_id: u8][payload_len: u16 BE][payload...]
```

Strings are length-prefixed: `[len: u16 BE][bytes...]`

## Usage

### Encoding

```rust
use voiceapp_protocol::Packet;

let packet = Packet::LoginRequest {
    request_id: 1,
    username: "alice".to_string(),
};

let bytes = packet.encode();
// Send bytes over TCP/UDP
```

### Decoding

```rust
use voiceapp_protocol::Packet;

let bytes = receive_from_network();
let (packet, size) = Packet::decode(&bytes)?;

match packet {
    Packet::LoginRequest { request_id, username } => {
        println!("User {} wants to log in (request_id: {})", username, request_id);
    }
    Packet::VoiceData { user_id, sequence, timestamp, data } => {
        // Process audio frame
    }
    _ => {}
}

// For TCP streaming, use size to advance buffer
buffer.drain(..size);
```

## Request/Response Correlation

All request and response packets include a `request_id: u64` field for proper request/response matching
### Client Usage

```rust
use std::sync::atomic::{AtomicU64, Ordering};

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

// Generate unique request ID
let request_id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

// Send request with ID
let request = Packet::JoinVoiceChannelRequest { request_id };
send(request.encode());

// Server echoes request_id in response
// Match response by request_id (not by packet type)
```

### Server Usage

Server simply echoes the `request_id` from the request in the corresponding response:

```rust
match packet {
    Packet::LoginRequest { request_id, username } => {
        // Process login...
        let response = Packet::LoginResponse {
            request_id,  // Echo the request_id
            id: user_id,
            voice_token,
            participants,
        };
        send(response.encode());
    }
    _ => {}
}
```