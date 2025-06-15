pub enum MessageType {
    Info,
    Warning,
    Error
}

pub fn info(message: &str, message_type: &MessageType) {
    match *message_type {
        MessageType::Info => println!("\x1b[0;32m * \x1b[0m{}", message),
        MessageType::Warning => println!("\x1b[0;33m * \x1b[0m{}", message),
        MessageType::Error => println!("\x1b[0;31m * \x1b[0m{}", message),
    }
}