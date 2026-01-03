# voiceapp-sdk

Voice communication client SDK with Opus encoding and adaptive jitter buffer.

## Architecture

- **Network Layer** - TCP client for management, UDP client for voice data
- **Voice Pipeline** - Opus encoder/decoder with sample rate conversion via rubato
- **Jitter Buffer** - NetEQ-based adaptive buffer for smooth playback

## Sequence Diagram

```
┌────────┐       ┌────────┐       ┌─────────────────┐       ┌────────────────┐
│  App   │       │  SDK   │       │ ManagementServer│       │VoiceRelayServer│
└───┬────┘       └───┬────┘       └───────┬─────────┘       └───────┬────────┘
    │                │                    │                         │
    │  connect()     │                    │                         │
    │───────────────>│                    │                         │
    │                │  TCP: Connect      │                         │
    │                │───────────────────>│                         │
    │                │  TCP: LoginReq     │                         │
    │                │───────────────────>│                         │
    │                │  LoginResp(token)  │                         │
    │                │<───────────────────│                         │
    │                │                    │                         │
    │                │  UDP: VoiceAuthReq ─────────────────────────>│
    │                │  UDP: VoiceAuthResp <────────────────────────│
    │  Ok(user_id)   │                    │                         │
    │<───────────────│                    │                         │
    │                │                    │                         │
    │  join_channel()│                    │                         │
    │───────────────>│                    │                         │
    │                │  TCP: JoinVoiceReq │                         │
    │                │───────────────────>│                         │
    │                │  JoinVoiceResp     │                         │
    │                │<───────────────────│                         │
    │                │                    │                         │
    │  send audio    │                    │                         │
    │───────────────>│                    │                         │
    │                │  [Resample→Encode] │                         │
    │                │  UDP: VoiceData ────────────────────────────>│
    │                │                    │                         │
    │                │  UDP: VoiceData <────────────────────────────│
    │                │  [Decode→Resample] │                         │
    │  receive audio │                    │                         │
    │<───────────────│                    │                         │
```

## Usage

```rust
use voiceapp_sdk::{Client, ClientEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    
    // Connect and authenticate (TCP + UDP)
    let user_id = client.connect("127.0.0.1:9001", "127.0.0.1:9002", "username").await?;
    
    // Subscribe to events
    let events = client.event_stream();
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            match event {
                ClientEvent::UserJoinedVoice { user_id } => { /* ... */ }
                ClientEvent::UserLeftVoice { user_id } => { /* ... */ }
                _ => {}
            }
        }
    });
    
    // Join voice channel
    client.join_channel().await?;
    
    // Setup voice I/O
    let input_tx = client.get_voice_input_sender(48000)?;  // Send audio samples
    let decoder = client.get_or_create_voice_output(other_user_id, 48000)?;  // Receive audio
    
    Ok(())
}
```

## API Reference

### Connection

| Method | Description |
|--------|-------------|
| `new()` | Create new client instance |
| `connect(mgmt_addr, voice_addr, username)` | Connect to servers, authenticate, returns `user_id` |
| `event_stream()` | Subscribe to server events, returns cloneable `Receiver<ClientEvent>` |

### Voice Channel

| Method | Description |
|--------|-------------|
| `join_channel()` | Join the voice channel |
| `leave_channel()` | Leave the voice channel |
| `send_mute_state(is_muted)` | Broadcast mute state to other participants |

### Voice I/O

| Method | Description |
|--------|-------------|
| `get_voice_input_sender(sample_rate)` | Returns `Sender<Vec<f32>>` for sending raw audio samples |
| `get_or_create_voice_output(user_id, sample_rate)` | Returns `Arc<Decoder>` for receiving user's audio |
| `remove_voice_output_for(user_id)` | Cleanup decoder when user leaves |
| `remove_all_voice_outputs()` | Cleanup all decoders |

### Utilities

| Method | Description |
|--------|-------------|
| `send_message(message)` | Send chat message |
| `ping()` | Ping server, returns RTT in milliseconds |
| `get_voice_stats()` | Returns `(bytes_sent, bytes_received)` |

## Events

| Event | Description |
|-------|-------------|
| `ParticipantsList` | Initial user list after login |
| `UserJoinedServer` | User connected to server |
| `UserLeftServer` | User disconnected |
| `UserJoinedVoice` | User joined voice channel |
| `UserLeftVoice` | User left voice channel |
| `UserSentMessage` | Chat message received |
| `UserMuteState` | User mute state changed |

## License

MIT
