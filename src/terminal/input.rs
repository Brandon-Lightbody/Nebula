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
    if key_event.state == winit::event::ElementState::Pressed {
        if let Some(key_char) = key_event.logical_key.to_text() {
            writer.write_all(key_char.as_bytes())?;
            state.cursor_x += 8.0;
            state.dirty = true;
        } else {
            match key_event.logical_key.as_ref() {
                winit::keyboard::Key::Named(named) => match named {
                    winit::keyboard::NamedKey::Backspace => {
                        writer.write_all(&[0x08])?;
                        state.cursor_x = (state.cursor_x - 8.0).max(0.0);
                        state.dirty = true;
                    }
                    winit::keyboard::NamedKey::Enter => {
                        writer.write_all(&[0x0D])?;
                        writer.write_all(&[0x0A])?;
                        state.cursor_x = 0.0;
                        state.cursor_y += 20.0;
                        state.dirty = true;
                    }
                    winit::keyboard::NamedKey::Tab => {
                        writer.write_all(&[0x09])?;
                        state.cursor_x += 8.0 * 4.0;
                        state.dirty = true;
                    }
                    winit::keyboard::NamedKey::Escape => {
                        writer.write_all(&[0x1B])?;
                        state.dirty = true;
                    }
                    winit::keyboard::NamedKey::ArrowUp => {
                        writer.write_all(b"\x1B[A")?;
                        state.cursor_y = (state.cursor_y - 20.0).max(0.0);
                        state.dirty = true;
                    }
                    winit::keyboard::NamedKey::ArrowDown => {
                        writer.write_all(b"\x1B[B")?;
                        state.cursor_y += 20.0;
                        state.dirty = true;
                    }
                    winit::keyboard::NamedKey::ArrowRight => {
                        writer.write_all(b"\x1B[C")?;
                        state.cursor_x += 8.0;
                        state.dirty = true;
                    }
                    winit::keyboard::NamedKey::ArrowLeft => {
                        writer.write_all(b"\x1B[D")?;
                        state.cursor_x = (state.cursor_x - 8.0).max(0.0);
                        state.dirty = true;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    writer.flush()?;
    Ok(())
}