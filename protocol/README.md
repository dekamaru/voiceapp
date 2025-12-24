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
    username: "alice".to_string(),
};

let bytes = packet.encode();
// Send bytes over TCP/UDP
```

### Decoding

```rust
use voiceapp_protocol::Packet;

let bytes = receive_from_network();
let packet = Packet::decode(&bytes)?;

match packet {
    Packet::LoginRequest { username } => {
        println!("User {} wants to log in", username);
    }
    Packet::VoiceData { sequence, timestamp, data, .. } => {
        // Process audio frame
    }
    _ => {}
}
```

## Packet Types

### Requests
- `LoginRequest` - Initial authentication
- `VoiceAuthRequest` - UDP voice channel auth
- `JoinVoiceChannelRequest` - Join voice channel
- `LeaveVoiceChannelRequest` - Leave voice channel
- `ChatMessageRequest` - Send chat message

### Responses
- `LoginResponse` - Auth result with participant list
- `VoiceAuthResponse` - UDP auth result
- `JoinVoiceChannelResponse` - Join result
- `LeaveVoiceChannelResponse` - Leave result
- `ChatMessageResponse` - Message send result

### Events
- `UserJoinedServer` - User connected
- `UserJoinedVoice` - User joined voice channel
- `UserLeftVoice` - User left voice channel
- `UserLeftServer` - User disconnected
- `UserSentMessage` - Chat message received

### UDP
- `VoiceData` - Audio frame (Opus encoded data)
