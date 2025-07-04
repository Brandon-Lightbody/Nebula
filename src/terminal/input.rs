use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{Key, NamedKey},
};
use std::io::Write;
use crate::terminal::TerminalState;
use anyhow::Result;

pub fn handle_input(
    key_event: &KeyEvent,
    writer: &mut dyn Write,
    state: &mut TerminalState
) -> Result<()> {
    if key_event.state == ElementState::Pressed {
        let mut input_bytes = Vec::new();
        
        // Handle both text and Character variants
        if let Some(text) = key_event.logical_key.to_text() {
            input_bytes.extend_from_slice(text.as_bytes());
        } else if let Key::Character(ch) = &key_event.logical_key {
            input_bytes.extend_from_slice(ch.as_bytes());
        }
        
        // Handle special keys
        match key_event.logical_key.as_ref() {
            Key::Named(named) => match named {
                NamedKey::Backspace => input_bytes.push(0x08),
                NamedKey::Enter => {
                    input_bytes.push(0x0D); // CR
                    input_bytes.push(0x0A); // LF
                },
                NamedKey::Tab => input_bytes.push(0x09),
                NamedKey::Escape => input_bytes.push(0x1B),
                NamedKey::ArrowUp => input_bytes.extend_from_slice(b"\x1B[A"),
                NamedKey::ArrowDown => input_bytes.extend_from_slice(b"\x1B[B"),
                NamedKey::ArrowRight => input_bytes.extend_from_slice(b"\x1B[C"),
                NamedKey::ArrowLeft => input_bytes.extend_from_slice(b"\x1B[D"),
                _ => (),
            },
            _ => (),
        }

        if !input_bytes.is_empty() {
            println!("Writing to PTY: {:?}", input_bytes);
            writer.write_all(&input_bytes)?;
            writer.flush()?;
            *state.shared_dirty.lock().unwrap() = true;
        }
    }
    Ok(())
}