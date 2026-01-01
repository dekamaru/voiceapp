use std::sync::Arc;
use iced::Task;
use voiceapp_sdk::Client;
use crate::application::Message;
use crate::state::State;

#[derive(Debug, Clone)]
pub enum VoiceCommand {
    Connect {
        management_addr: String,
        voice_addr: String,
        username: String,
    },
    JoinVoiceChannel,
    LeaveVoiceChannel,
    SendChatMessage(String),
    Ping,
    GetVoiceStats,
}

#[derive(Debug, Clone)]
pub enum VoiceCommandResult {
    Connect(Result<(u64, String, String), String>),  // Ok((user_id, address, username))
    JoinVoiceChannel(Result<(), String>),
    LeaveVoiceChannel(Result<(), String>),
    SendChatMessage(Result<(), String>),
    Ping(Result<u64, String>),  // RTT in milliseconds
    VoiceStats(u64, u64),  // (bytes_sent, bytes_received)
}

pub struct VoiceClientState {
    voice_client: Arc<Client>,
}

impl VoiceClientState {
    pub fn new(voice_client: Arc<Client>) -> Self {
        Self { voice_client }
    }

    fn handle_voice_command(&self, command: VoiceCommand) -> Task<Message> {
        let client = self.voice_client.clone();

        match command {
            VoiceCommand::Connect {
                management_addr,
                voice_addr,
                username,
            } => {
                // Extract server address from management_addr (remove port)
                let server_addr = management_addr.split(':').next().unwrap_or("").to_string();
                let username_clone = username.clone();

                Task::perform(
                    async move { client.connect(&management_addr, &voice_addr, &username).await },
                    |result| {
                        Message::VoiceCommandResult(VoiceCommandResult::Connect(
                            result
                                .map(|user_id| (user_id, server_addr, username_clone))
                                .map_err(|e| e.to_string()),
                        ))
                    },
                )
            }
            VoiceCommand::JoinVoiceChannel => Task::perform(
                async move { client.join_channel().await },
                |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::LeaveVoiceChannel => Task::perform(
                async move { client.leave_channel().await },
                |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::SendChatMessage(message) => Task::perform(
                async move { client.send_message(&message).await },
                |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::SendChatMessage(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::Ping => Task::perform(
                async move { client.ping().await },
                |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::Ping(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::GetVoiceStats => {
                let (sent, received) = client.get_voice_stats();
                Task::done(Message::VoiceCommandResult(VoiceCommandResult::VoiceStats(sent, received)))
            },
        }
    }
}

impl State for VoiceClientState {
    fn init(&mut self) -> Task<Message> {
        Task::run(self.voice_client.event_stream(), |e| Message::ServerEventReceived(e))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ExecuteVoiceCommand(command) => self.handle_voice_command(command),
            Message::MuteInput(muted) => {
                let voice_client = self.voice_client.clone();
                // TODO: error handling
                Task::future(async move { voice_client.send_mute_state(muted).await; Message::None })
            }
            _ => Task::none()
        }
    }
}