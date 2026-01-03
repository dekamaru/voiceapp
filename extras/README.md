# VoiceApp Extras

Additional utilities and tools for the VoiceApp voice communication platform.

## Music Bot

A command-line tool for streaming WAV audio files to a voice channel.

### Building

```bash
cargo build --release -p voiceapp-extras
```

### Usage

```bash
music_bot <wav_file>
```

### WAV File Requirements

The audio file must match the following specifications:

| Property    | Required Value |
|-------------|----------------|
| Sample Rate | 48000 Hz       |
| Channels    | Mono (1)       |
| Bit Depth   | 16-bit         |

### Example

```bash
# Stream a music file to the voice channel
music_bot music.wav
```

### Options

| Option              | Description                  | Default         |
|---------------------|------------------------------|-----------------|
| `--server`          | Management server address    | `127.0.0.1:9001`|
| `--voice-server`    | Voice relay server address   | `127.0.0.1:9002`|

### Examples

```bash
# Use default server addresses
music_bot music.wav

# Specify custom server addresses
music_bot --server 192.168.1.100:9001 --voice-server 192.168.1.100:9002 music.wav
```
