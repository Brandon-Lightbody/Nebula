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
        // Handle printable characters
        if let Some(key_char) = key_event.logical_key.to_text() {
            println!("Writing to PTY: {}", key_char);
            writer.write_all(key_char.as_bytes())?;
            // Update shared cursor position
            let mut cursor_x = state.cursor_x.lock().unwrap();
            *cursor_x += 8.0;
            // Set shared dirty flag
            *state.shared_dirty.lock().unwrap() = true;
        } 
        // Handle special keys
        else {
            match key_event.logical_key.as_ref() {
                Key::Named(named) => match named {
                    NamedKey::Backspace => {
                        println!("Writing to PTY: Backspace");
                        writer.write_all(&[0x08])?;
                        let mut cursor_x = state.cursor_x.lock().unwrap();
                        *cursor_x = (*cursor_x - 8.0).max(0.0);
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    NamedKey::Enter => {
                        println!("Writing to PTY: Enter");
                        writer.write_all(&[0x0D])?;  // CR
                        writer.write_all(&[0x0A])?;  // LF
                        let mut cursor_x = state.cursor_x.lock().unwrap();
                        let mut cursor_y = state.cursor_y.lock().unwrap();
                        *cursor_x = 0.0;
                        *cursor_y += 20.0;
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    NamedKey::Tab => {
                        println!("Writing to PTY: Tab");
                        writer.write_all(&[0x09])?;
                        let mut cursor_x = state.cursor_x.lock().unwrap();
                        *cursor_x += 8.0 * 4.0;
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    NamedKey::Escape => {
                        println!("Writing to PTY: Escape");
                        writer.write_all(&[0x1B])?;
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    NamedKey::ArrowUp => {
                        println!("Writing to PTY: Up Arrow");
                        writer.write_all(b"\x1B[A")?;
                        let mut cursor_y = state.cursor_y.lock().unwrap();
                        *cursor_y = (*cursor_y - 20.0).max(0.0);
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    NamedKey::ArrowDown => {
                        println!("Writing to PTY: Down Arrow");
                        writer.write_all(b"\x1B[B")?;
                        let mut cursor_y = state.cursor_y.lock().unwrap();
                        *cursor_y += 20.0;
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    NamedKey::ArrowRight => {
                        println!("Writing to PTY: Right Arrow");
                        writer.write_all(b"\x1B[C")?;
                        let mut cursor_x = state.cursor_x.lock().unwrap();
                        *cursor_x += 8.0;
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    NamedKey::ArrowLeft => {
                        println!("Writing to PTY: Left Arrow");
                        writer.write_all(b"\x1B[D")?;
                        let mut cursor_x = state.cursor_x.lock().unwrap();
                        *cursor_x = (*cursor_x - 8.0).max(0.0);
                        *state.shared_dirty.lock().unwrap() = true;
                    }
                    _ => {}
                },
                Key::Character(ch) => {
                    // Handle characters that might not be caught by to_text()
                    println!("Writing to PTY: {}", ch);
                    writer.write_all(ch.as_bytes())?;
                    let mut cursor_x = state.cursor_x.lock().unwrap();
                    *cursor_x += 8.0;
                    *state.shared_dirty.lock().unwrap() = true;
                }
                _ => {}
            }
        }
        // Always flush after handling input
        writer.flush()?;
    }
    Ok(())
}