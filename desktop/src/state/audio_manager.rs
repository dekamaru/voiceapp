use std::collections::HashSet;
use iced::Task;
use tracing::{error, info};
use voiceapp_sdk::ClientEvent;
use crate::application::Message;
use crate::audio::AudioManager;
use crate::view::settings::SettingsPageMessage;
use crate::state::State;
use crate::state::voice_client::VoiceCommandResult;

pub struct AudioManagerState {
    audio_manager: AudioManager,
    user_id: u64,
    users_in_voice: HashSet<u64>,
}

impl AudioManagerState {
    pub fn new(audio_manager: AudioManager) -> Self {
        Self { audio_manager, user_id: 0, users_in_voice: HashSet::new() }
    }
}

impl State for AudioManagerState {
    fn init(&mut self) -> Task<Message> {
        if let Err(e) = self.audio_manager.init_notification_player() {
            error!("Failed to init audio manager: {}", e);
        }
        
        Task::none()
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::VoiceCommandResult(VoiceCommandResult::Connect(Ok((user_id, _, _)))) => {
                self.user_id = user_id
            },
            Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(Ok(()))) => {
                self.audio_manager.play_notification("join_voice");

                // Start recording
                if let Err(e) = self.audio_manager.start_recording() {
                    error!("Failed to start recording: {}", e);
                }

                // Create output streams for all users currently in voice
                for user_id in self.users_in_voice.clone() {
                    if let Err(e) = self.audio_manager.create_output_stream_for_user(user_id) {
                        error!("Failed to create output stream for user {}: {}", user_id, e);
                    }
                };

                self.users_in_voice.insert(self.user_id);
            },
            Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(Ok(()))) => {
                self.audio_manager.play_notification("leave_voice");
                self.audio_manager.stop_recording();
                self.audio_manager.remove_all_output_streams();
                self.users_in_voice.remove(&self.user_id);
            },
            Message::ServerEventReceived(ClientEvent::UserJoinedVoice { user_id }) => {
                // Create output stream for new user in voice
                self.users_in_voice.insert(user_id);

                if self.users_in_voice.contains(&self.user_id) {
                    self.audio_manager.play_notification("join_voice");
                    if let Err(e) = self.audio_manager.create_output_stream_for_user(user_id) {
                        error!("Failed to create output stream for user {}: {}", user_id, e);
                    };
                }
            },
            Message::ServerEventReceived(ClientEvent::UserLeftVoice { user_id }) => {
                if self.users_in_voice.contains(&self.user_id) {
                    self.audio_manager.play_notification("leave_voice");
                    self.audio_manager.remove_output_stream_for_user(user_id);
                }

                self.users_in_voice.remove(&user_id);
            },
            Message::SettingsPage(SettingsPageMessage::SelectInputDevice(device_id)) => {
                if self.users_in_voice.contains(&self.user_id) {
                    self.audio_manager.stop_recording();
                    // TODO: error handling
                    let _ = self.audio_manager.start_recording();
                }

                info!("Selected input device: {}", device_id);
            },
            Message::SettingsPage(SettingsPageMessage::SelectOutputDevice(device_id)) => {
                if let Err(e) = self.audio_manager.init_notification_player() {
                    error!("failed to initialize notification player: {}", e);
                };

                self.audio_manager.play_notification("unmute");

                if self.users_in_voice.contains(&self.user_id) {
                    self.audio_manager.remove_all_output_streams();
                    // Create output streams for all users currently in voice except user itself
                    for user_id in self.users_in_voice.clone() {
                        if user_id != self.user_id {
                            if let Err(e) = self.audio_manager.create_output_stream_for_user(user_id) {
                                error!("Failed to create output stream for user {}: {}", user_id, e);
                            }
                        }

                    };
                }

                info!("Selected output device: {}", device_id);
            },
            Message::MuteInput(muted) => {
                if muted {
                    self.audio_manager.mute_input();
                    self.audio_manager.play_notification("mute");
                } else {
                    self.audio_manager.unmute_input();
                    self.audio_manager.play_notification("unmute");
                }
            }
            _ => {}
        }

        Task::none()
    }
}