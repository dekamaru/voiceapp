use iced::Task;
use crate::application::Message;

pub trait View {
    fn update(&mut self, message: Message) -> Task<Message>;
    fn render(&self) -> iced::Element<'_, Message>;
    fn on_open(&mut self) -> Task<Message> { Task::none() }
    fn on_close(&mut self) -> Task<Message> { Task::none() }
}