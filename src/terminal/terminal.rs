use anyhow::Result;
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use portable_pty::{CommandBuilder, Child, NativePtySystem, PtySize, PtySystem};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread,
};
use crate::terminal::config::{FONT_SIZE, LINE_HEIGHT};

pub struct Terminal {
    pub font_system: Arc<Mutex<FontSystem>>,
    pub buffer: Arc<Mutex<Buffer>>,
    pub text_content: Arc<Mutex<String>>,
}

impl Terminal {
    pub fn new() -> Self {
        let font_system = Arc::new(Mutex::new(FontSystem::new()));
        let metrics = Metrics::new(FONT_SIZE, LINE_HEIGHT);
        let mut buffer = Buffer::new(&mut font_system.lock().unwrap(), metrics);
        buffer.set_text(
            &mut font_system.lock().unwrap(), 
            "Nebula\n$ ", 
            &Attrs::new(), 
            Shaping::Advanced
        );
        
        let buffer = Arc::new(Mutex::new(buffer));
        {
            let mut buffer_lock = buffer.lock().unwrap();
            buffer_lock.set_size(
                &mut font_system.lock().unwrap(),
                Some(1600.0),
                Some(900.0),
            );
        }

        let text_content = Arc::new(Mutex::new(String::from("Nebula\n$ ")));
        
        Self {
            font_system,
            buffer,
            text_content,
        }
    }

    pub fn spawn_pty(&self) -> Result<(Arc<Mutex<dyn Write + Send>>, Arc<Mutex<Box<dyn Child + Send>>>)> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        
        println!("PTY created successfully");

        // For Unix systems, set raw mode
        #[cfg(unix)]
        {
            use portable_pty::MasterPty;
            if let Err(e) = pair.master.set_raw_mode() {
                eprintln!("Failed to set PTY raw mode: {}", e);
            } else {
                println!("PTY raw mode enabled");
            }
        }

        // Create a simple command that stays open
        let mut cmd = if cfg!(target_os = "windows") {
            let mut cmd = CommandBuilder::new("cmd.exe");
            cmd.arg("/K");
            cmd
        } else {
            let mut cmd = CommandBuilder::new("bash");
            cmd.arg("-i");
            cmd
        };
        
        // Use minimal environment
        cmd.env_clear();
        if cfg!(target_os = "windows") {
            cmd.env("SystemRoot", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string()));
            cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
            cmd.env("COMSPEC", std::env::var("COMSPEC").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string()));
        } else {
            cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
            cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
            cmd.env("TERM", "xterm-256color");
        };
        
        println!("Spawning command: {:?}", cmd);
        let child: Box<dyn Child + Send> = pair.slave.spawn_command(cmd)?;
        println!("Child process spawned: {:?}", child);
        
        let child_ref = Arc::new(Mutex::new(child));
        
        // Keep the master PTY alive by storing it in an Arc
        let master = pair.master;
        let master_ref = Arc::new(Mutex::new(master));
        
        // Clone the reader and writer from the master
        let reader = master_ref.lock().unwrap().try_clone_reader()?;
        let writer = master_ref.lock().unwrap().take_writer()?;
        
        let buffer_clone = Arc::clone(&self.buffer);
        let font_system_clone = Arc::clone(&self.font_system);
        let text_content_clone = Arc::clone(&self.text_content);
        let child_clone = Arc::clone(&child_ref);
        let master_clone = Arc::clone(&master_ref);
        
        thread::spawn(move || {
            println!("PTY reader thread started");
            let mut reader = reader;
            let mut buffer = [0; 1024];
            
            // Buffer to accumulate incomplete UTF-8 sequences
            let mut utf8_buffer = Vec::new();
            
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        println!("PTY reader reached EOF");
                        // Check why the process exited
                        let mut child = child_clone.lock().unwrap();
                        if let Ok(Some(exit_status)) = child.try_wait() {
                            println!("Child process exited with status: {:?}", exit_status);
                        } else {
                            println!("Child process status unknown");
                        }
                        break;
                    }
                    Ok(n) => {
                        let data = &buffer[..n];
                        
                        // Print to console for debugging
                        println!("PTY OUTPUT ({} bytes): {:?}", n, data);
                        
                        // Accumulate data into UTF-8 buffer
                        utf8_buffer.extend_from_slice(data);
                        
                        // Try to convert to UTF-8
                        match String::from_utf8(utf8_buffer.clone()) {
                            Ok(valid_str) => {
                                println!("PTY OUTPUT (str): {}", valid_str);
                                
                                let mut text = text_content_clone.lock().unwrap();
                                text.push_str(&valid_str);
                                
                                if let Ok(mut buffer_lock) = buffer_clone.lock() {
                                    if let Ok(mut fs) = font_system_clone.lock() {
                                        buffer_lock.set_text(
                                            &mut fs, 
                                            &text, 
                                            &Attrs::new(), 
                                            Shaping::Advanced
                                        );
                                        buffer_lock.shape_until_scroll(&mut fs, true);
                                        println!("Buffer updated with new text");
                                    } else {
                                        eprintln!("Failed to lock font system");
                                    }
                                } else {
                                    eprintln!("Failed to lock buffer");
                                }
                                
                                // Clear the buffer after successful conversion
                                utf8_buffer.clear();
                            }
                            Err(e) => {
                                let utf8_error = e.utf8_error();
                                let valid_up_to = utf8_error.valid_up_to();
                                
                                if valid_up_to > 0 {
                                    // Extract valid part
                                    let valid_part = String::from_utf8_lossy(&utf8_buffer[..valid_up_to]).to_string();
                                    println!("PTY OUTPUT (partial str): {}", valid_part);
                                    
                                    let mut text = text_content_clone.lock().unwrap();
                                    text.push_str(&valid_part);
                                    
                                    if let Ok(mut buffer_lock) = buffer_clone.lock() {
                                        if let Ok(mut fs) = font_system_clone.lock() {
                                            buffer_lock.set_text(
                                                &mut fs, 
                                                &text, 
                                                &Attrs::new(), 
                                                Shaping::Advanced
                                            );
                                            buffer_lock.shape_until_scroll(&mut fs, true);
                                            println!("Buffer updated with partial text");
                                        }
                                    }
                                    
                                    // Keep only the invalid part for next iteration
                                    utf8_buffer = utf8_buffer[valid_up_to..].to_vec();
                                }
                                // If no valid part, keep the entire buffer for next read
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("PTY read error: {}", e);
                        // Check why the process exited
                        let mut child = child_clone.lock().unwrap();
                        if let Ok(Some(exit_status)) = child.try_wait() {
                            println!("Child process exited with status: {:?}", exit_status);
                        }
                        break;
                    }
                }
            }
            
            // Keep master_ref alive until the end of the thread
            let _unused = master_clone.lock().unwrap();
            println!("PTY reader thread exiting");
        });

        println!("Returning PTY writer and child reference");
        Ok((Arc::new(Mutex::new(writer)), child_ref))
    }
}