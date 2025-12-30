use iced::Task;
use crate::application::Message;

pub trait State {
    fn init(&mut self) -> Task<Message> { Task::none() }
    fn update(&mut self, message: Message) -> Task<Message>;
}
