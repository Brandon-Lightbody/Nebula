use anyhow::Result;
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use portable_pty::{CommandBuilder, Child, NativePtySystem, PtySize, PtySystem};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread,
    collections::VecDeque,
};
use vte::{Params, Perform};
use crate::terminal::SwashCache;
use cosmic_text::Family;

pub const FONT_SIZE: f32 = 14.0;
pub const LINE_HEIGHT: f32 = 20.0;
pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 24;

#[derive(Debug, Clone)]
struct TerminalCell {
    character: char,
    // Add attributes later: bold, italic, color, etc.
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self { character: ' ' }
    }
}

struct TerminalGrid {
    rows: usize,
    cols: usize,
    cells: Vec<Vec<TerminalCell>>,
    cursor_x: usize,
    cursor_y: usize,
    scrollback: VecDeque<String>,
    scroll_offset: usize,
    dirty: bool,
}

impl TerminalGrid {
    fn new(rows: usize, cols: usize) -> Self {
        let mut cells = Vec::with_capacity(rows);
        for _ in 0..rows {
            let mut row = Vec::with_capacity(cols);
            for _ in 0..cols {
                row.push(TerminalCell::default());
            }
            cells.push(row);
        }
        
        Self {
            rows,
            cols,
            cells,
            cursor_x: 0,
            cursor_y: 0,
            scrollback: VecDeque::new(),
            scroll_offset: 0,
            dirty: true,
        }
    }

    fn clear_screen(&mut self) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                self.cells[row][col] = TerminalCell::default();
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.dirty = true;
    }

    fn clear_line(&mut self, from: usize) {
        let row = self.cursor_y;
        if row < self.rows {
            for col in from..self.cols {
                self.cells[row][col] = TerminalCell::default();
            }
            self.dirty = true;
        }
    }

    fn newline(&mut self) {
        if self.cursor_y == self.rows - 1 {
            self.scroll_up();
        } else {
            self.cursor_y += 1;
        }
        self.cursor_x = 0;
        self.dirty = true;
    }

    fn carriage_return(&mut self) {
        self.cursor_x = 0;
        self.dirty = true;
    }

    fn backspace(&mut self) {
        if self.cursor_x > 0 {
            self.cursor_x -= 1;
            self.cells[self.cursor_y][self.cursor_x] = TerminalCell::default();
            self.dirty = true;
        }
    }

    fn scroll_up(&mut self) {
        // Collect top line as string
        let top_line: String = self.cells[0]
            .iter()
            .map(|cell| cell.character)
            .collect();
        self.scrollback.push_back(top_line);
        
        // Shift lines up
        for row in 0..self.rows - 1 {
            for col in 0..self.cols {
                self.cells[row][col] = self.cells[row + 1][col].clone();
            }
        }
        
        // Clear bottom line
        for col in 0..self.cols {
            self.cells[self.rows - 1][col] = TerminalCell::default();
        }
        self.dirty = true;
    }

    fn scroll_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            if let Some(bottom_line) = self.scrollback.pop_back() {
                // Shift lines down
                for row in (1..self.rows).rev() {
                    for col in 0..self.cols {
                        self.cells[row][col] = self.cells[row - 1][col].clone();
                    }
                }
                
                // Set top line from scrollback
                for (col, c) in bottom_line.chars().enumerate().take(self.cols) {
                    self.cells[0][col] = TerminalCell { character: c };
                }
                self.dirty = true;
            }
        }
    }

    fn move_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = x.min(self.cols - 1);
        self.cursor_y = y.min(self.rows - 1);
        self.dirty = true;
    }

    fn move_cursor_relative(&mut self, dx: i32, dy: i32) {
        let new_x = (self.cursor_x as i32 + dx).max(0) as usize;
        let new_y = (self.cursor_y as i32 + dy).max(0) as usize;
        self.move_cursor(new_x, new_y);
    }

    fn print_char(&mut self, c: char) {
        if self.cursor_y < self.rows && self.cursor_x < self.cols {
            self.cells[self.cursor_y][self.cursor_x] = TerminalCell { character: c };
            self.cursor_x += 1;
            self.dirty = true;
        }
        
        // Only wrap when at column boundary
        if self.cursor_x >= self.cols {
            self.carriage_return();
            self.newline();
        }
    }

    fn print_str(&mut self, s: &str) {
        for c in s.chars() {
            self.print_char(c);
        }
    }

    fn to_string(&self) -> String {
        let mut output = String::new();
        
        // Add scrollback lines
        for line in self.scrollback.iter().skip(self.scroll_offset) {
            output.push_str(line);
            output.push('\n');
        }
        
        // Add current screen content
        for row in 0..self.rows {
            let line: String = self.cells[row]
                .iter()
                .map(|cell| cell.character)
                .collect();
            output.push_str(&line);
            if row < self.rows - 1 {
                output.push('\n');
            }
        }
        
        output
    }
}

struct TerminalPerformer {
    grid: TerminalGrid,
    writer: Arc<Mutex<dyn Write + Send>>,  // Add writer for escape sequence responses
}

impl TerminalPerformer {
    fn new(rows: usize, cols: usize, writer: Arc<Mutex<dyn Write + Send>>) -> Self {
        Self {
            grid: TerminalGrid::new(rows, cols),
            writer,
        }
    }
}

impl Perform for TerminalPerformer {
    fn print(&mut self, c: char) {
        self.grid.print_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => self.grid.backspace(),    // Backspace
            0x09 => self.grid.print_str("    "), // Tab (4 spaces)
            0x0A => self.grid.newline(),      // Line feed
            0x0D => self.grid.carriage_return(), // Carriage return
            0x0C => self.grid.clear_screen(), // Form feed (clear screen)
            _ => (),
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let get_param = |index: usize| -> usize {
            params.into_iter().nth(index)
                .and_then(|p| p.first().copied())
                .unwrap_or(1) as usize
        };

        match action {
            // Cursor movement
            'A' => self.grid.move_cursor_relative(0, -(get_param(0) as i32)), // Up
            'B' => self.grid.move_cursor_relative(0, get_param(0) as i32),   // Down
            'C' => self.grid.move_cursor_relative(get_param(0) as i32, 0),   // Right
            'D' => self.grid.move_cursor_relative(-(get_param(0) as i32), 0), // Left
            'H' | 'f' => { // Cursor position
                let row = get_param(0).saturating_sub(1);
                let col = get_param(1).saturating_sub(1);
                self.grid.move_cursor(col, row);
            },
            
            // Screen clearing
            'J' => match get_param(0) {
                0 => { // Clear from cursor to end of screen
                    self.grid.clear_line(self.grid.cursor_x);
                    for y in self.grid.cursor_y + 1..self.grid.rows {
                        for x in 0..self.grid.cols {
                            self.grid.cells[y][x] = TerminalCell::default();
                        }
                    }
                },
                1 => { // Clear from beginning to cursor
                    for y in 0..self.grid.cursor_y {
                        for x in 0..self.grid.cols {
                            self.grid.cells[y][x] = TerminalCell::default();
                        }
                    }
                    self.grid.clear_line(0);
                },
                2 => self.grid.clear_screen(), // Clear entire screen
                _ => (),
            },
            'K' => match get_param(0) {
                0 => self.grid.clear_line(self.grid.cursor_x), // Clear to end of line
                1 => self.grid.clear_line(0), // Clear from beginning of line
                2 => { // Clear entire line
                    for x in 0..self.grid.cols {
                        self.grid.cells[self.grid.cursor_y][x] = TerminalCell::default();
                    }
                },
                _ => (),
            },
            
            // Scrolling
            'S' => { // Scroll up
                for _ in 0..get_param(0) {
                    self.grid.scroll_up();
                }
            },
            'T' => { // Scroll down
                for _ in 0..get_param(0) {
                    self.grid.scroll_down();
                }
            },
            
            // Character deletion
            'P' => { // Delete character
                let row = self.grid.cursor_y;
                let start = self.grid.cursor_x;
                let count = get_param(0);
                let end = (start + count).min(self.grid.cols);
                
                // Shift characters left
                for x in start..(self.grid.cols - count) {
                    if x + count < self.grid.cols {
                        self.grid.cells[row][x] = self.grid.cells[row][x + count].clone();
                    }
                }
                
                // Clear remaining characters
                for x in (self.grid.cols - count)..self.grid.cols {
                    self.grid.cells[row][x] = TerminalCell::default();
                }
            },
            
            // Handle Device Status Report (DSR)
            'n' => {
                if get_param(0) == 6 {
                    // Respond with cursor position report
                    let response = format!(
                        "\x1B[{};{}R",
                        self.grid.cursor_y + 1,
                        self.grid.cursor_x + 1
                    );
                    if let Ok(mut w) = self.writer.lock() {
                        let _ = w.write_all(response.as_bytes());
                        let _ = w.flush();
                        println!("Responded to DSR: {}", response);
                    }
                }
            }
            
            _ => (),
        }
    }

    // Required trait methods
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

pub struct Terminal {
    pub font_system: Arc<Mutex<FontSystem>>,
    pub buffer: Arc<Mutex<Buffer>>,
    pub text_content: Arc<Mutex<String>>,
    pub cursor_x: Arc<Mutex<f32>>,
    pub cursor_y: Arc<Mutex<f32>>,
    pub dirty: Arc<Mutex<bool>>,
    pub cols: Arc<Mutex<usize>>,
    pub rows: Arc<Mutex<usize>>,
    pub swash_cache: Arc<Mutex<SwashCache>>,
}

impl Terminal {
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();
        // Load system fonts for proper rendering
        font_system.db_mut().load_system_fonts();
        
        let metrics = Metrics::new(FONT_SIZE, LINE_HEIGHT);
        let mut buffer = Buffer::new(&mut font_system, metrics);
        
        let initial_text = "Nebula Terminal\n$ ";
        buffer.set_text(
            &mut font_system, 
            initial_text, 
            &Attrs::new(), 
            Shaping::Advanced
        );
        
        let buffer = Arc::new(Mutex::new(buffer));
        {
            let mut buffer_lock = buffer.lock().unwrap();
            buffer_lock.set_size(
                &mut font_system,
                Some(1600.0),
                Some(900.0),
            );
        }

        let text_content = Arc::new(Mutex::new(String::from(initial_text)));
        // After "$ " (2 characters * FONT_SIZE) at line 1
        let cursor_x = Arc::new(Mutex::new(2.0 * FONT_SIZE));
        let cursor_y = Arc::new(Mutex::new(1.0 * LINE_HEIGHT));
        let dirty = Arc::new(Mutex::new(true));
        let cols = Arc::new(Mutex::new(DEFAULT_COLS as usize));
        let rows = Arc::new(Mutex::new(DEFAULT_ROWS as usize));
        let swash_cache = Arc::new(Mutex::new(SwashCache::new()));
        
        Self {
            font_system: Arc::new(Mutex::new(font_system)),
            buffer,
            text_content,
            cursor_x,
            cursor_y,
            dirty,
            cols,
            rows,
            swash_cache
        }
    }

    pub fn spawn_pty(&self) -> Result<(Arc<Mutex<dyn Write + Send>>, Arc<Mutex<Box<dyn Child + Send>>>)> {
    let pty_system = NativePtySystem::default();
    let pair = pty_system.openpty(PtySize {
        rows: DEFAULT_ROWS,
        cols: DEFAULT_COLS,
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

    // Create a command with proper shell initialization
    let mut cmd = if cfg!(target_os = "windows") {
        let mut cmd = CommandBuilder::new("cmd.exe");
        cmd.arg("/K");
        cmd.env("PROMPT", "$G$S"); // Simplify prompt
        cmd
    } else {
        let mut cmd = CommandBuilder::new("bash");
        // Use --login for proper initialization
        cmd.args(&["--login", "-i"]);
        cmd
    };
    
    // Set essential environment variables
    //cmd.env_clear();
    if cfg!(target_os = "windows") {
        cmd.env("SystemRoot", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string()));
        cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
        cmd.env("COMSPEC", std::env::var("COMSPEC").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string()));
        cmd.env("TEMP", std::env::var("TEMP").unwrap_or_else(|_| "C:\\Windows\\Temp".to_string()));
    } else {
        cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
        cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("SHELL", std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()));
        cmd.env("USER", std::env::var("USER").unwrap_or_default());
        cmd.env("LANG", "en_US.UTF-8");
    };
    
    println!("Spawning command: {:?}", cmd);
    let child: Box<dyn Child + Send> = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Failed to spawn child process: {}", e);
            return Err(e.into());
        }
    };
    println!("Child process spawned: {:?}", child);
    
    let child_ref = Arc::new(Mutex::new(child));
    let master = pair.master;
    let master_ref = Arc::new(Mutex::new(master));
    let reader = master_ref.lock().unwrap().try_clone_reader()?;
    let writer = master_ref.lock().unwrap().take_writer()?;
    
    // Clone shared state
    let buffer_clone = Arc::clone(&self.buffer);
    let font_system_clone = Arc::clone(&self.font_system);
    let text_content_clone = Arc::clone(&self.text_content);
    let cursor_x_clone = Arc::clone(&self.cursor_x);
    let cursor_y_clone = Arc::clone(&self.cursor_y);
    let dirty_clone = Arc::clone(&self.dirty);
    let cols_clone = Arc::clone(&self.cols);
    let rows_clone = Arc::clone(&self.rows);
    let swash_cache_clone = Arc::clone(&self.swash_cache);
    
    // Create inner references that can be cloned in the loop
    let child_ref_inner = child_ref.clone();
    let master_ref_inner = master_ref.clone();

    // Create a writer for escape sequence responses
    let writer_arc = Arc::new(Mutex::new(writer));
    let response_writer = Arc::clone(&writer_arc);

    thread::spawn(move || {
        println!("PTY reader thread started");
        let mut reader = reader;
        let mut buffer = [0; 4096];
        let mut parser = vte::Parser::new();
        
        let cols = *cols_clone.lock().unwrap();
        let rows = *rows_clone.lock().unwrap();
        let mut performer = TerminalPerformer::new(rows, cols, response_writer);
        
        performer.grid.print_str("Nebula Terminal\n$ ");
        
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    println!("Shell exited, restarting...");
                    performer.grid.print_str("\n[Shell exited, restarting...]\n");
                    
                    let new_pair = match pty_system.openpty(PtySize {
                        rows: DEFAULT_ROWS,
                        cols: DEFAULT_COLS,
                        pixel_width: 0,
                        pixel_height: 0,
                    }) {
                        Ok(pair) => pair,
                        Err(e) => {
                            performer.grid.print_str(&format!("\n[Failed to create PTY: {}]\n", e));
                            break;
                        }
                    };
                    
                    let mut cmd = if cfg!(target_os = "windows") {
                        let mut cmd = CommandBuilder::new("cmd.exe");
                        cmd.arg("/K");
                        cmd.env("PROMPT", "$G$S");
                        cmd
                    } else {
                        let mut cmd = CommandBuilder::new("bash");
                        cmd.args(&["--login", "-i"]);
                        cmd
                    };
                    
                    cmd.env_clear();
                    if cfg!(target_os = "windows") {
                        cmd.env("SystemRoot", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string()));
                        cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
                        cmd.env("COMSPEC", std::env::var("COMSPEC").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string()));
                        cmd.env("TEMP", std::env::var("TEMP").unwrap_or_else(|_| "C:\\Windows\\Temp".to_string()));
                    } else {
                        cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
                        cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
                        cmd.env("TERM", "xterm-256color");
                        cmd.env("SHELL", std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()));
                        cmd.env("USER", std::env::var("USER").unwrap_or_default());
                        cmd.env("LANG", "en_US.UTF-8");
                    };
                    
                    let new_child = match new_pair.slave.spawn_command(cmd) {
                        Ok(child) => child,
                        Err(e) => {
                            performer.grid.print_str(&format!("\n[Failed to spawn shell: {}]\n", e));
                            break;
                        }
                    };
                    
                    // Update references using inner clones
                    *child_ref_inner.lock().unwrap() = new_child;
                    *master_ref_inner.lock().unwrap() = new_pair.master;
                    
                    // Recreate reader from new master
                    reader = match master_ref.lock().unwrap().try_clone_reader() {
                        Ok(reader) => reader,
                        Err(e) => {
                            performer.grid.print_str(&format!("\n[Failed to clone reader: {}]\n", e));
                            break;
                        }
                    };
                    
                    // Reset terminal state
                    performer.grid.clear_screen();
                    performer.grid.cursor_x = 0;
                    performer.grid.cursor_y = 0;
                    performer.grid.scrollback.clear();
                    performer.grid.scroll_offset = 0;
                    performer.grid.dirty = true;
                    
                    // Print fresh prompt
                    performer.grid.print_str("Nebula Terminal\n$ ");
                    
                    // Update state to reflect new prompt
                    let new_text = performer.grid.to_string();
                    let cursor_x = performer.grid.cursor_x as f32 * FONT_SIZE;
                    let cursor_y = performer.grid.cursor_y as f32 * LINE_HEIGHT;
                    
                    {
                        let mut text_lock = text_content_clone.lock().unwrap();
                        *text_lock = new_text.clone();
                    }
                    
                    {
                        let mut buffer_lock = buffer_clone.lock().unwrap();
                        if let Ok(mut fs) = font_system_clone.lock() {
                            buffer_lock.set_text(
                                &mut fs, 
                                &new_text, 
                                &Attrs::new().family(Family::Monospace),
                                Shaping::Advanced
                            );
                            buffer_lock.shape_until_scroll(&mut fs, true);
                        }
                    }
                    
                    *cursor_x_clone.lock().unwrap() = cursor_x;
                    *cursor_y_clone.lock().unwrap() = cursor_y;
                    *dirty_clone.lock().unwrap() = true;
                }
                Ok(n) => {
                    let data = &buffer[..n];
                    println!("PTY received {} bytes: {:?}", n, data);
                    
                    for &byte in data {
                        parser.advance(&mut performer, &[byte]);
                    }
                    
                    if performer.grid.dirty {
                        println!("Grid dirty - cursor: ({}, {})", 
                            performer.grid.cursor_x, performer.grid.cursor_y);
                        println!("Grid content:\n{}", performer.grid.to_string());
                        
                        let new_text = performer.grid.to_string();
                        let cursor_x = performer.grid.cursor_x as f32 * FONT_SIZE;
                        let cursor_y = performer.grid.cursor_y as f32 * LINE_HEIGHT;
                        
                        {
                            let mut text_lock = text_content_clone.lock().unwrap();
                            *text_lock = new_text.clone();
                        }
                        
                        {
                            let mut buffer_lock = buffer_clone.lock().unwrap();
                            if let Ok(mut fs) = font_system_clone.lock() {
                                buffer_lock.set_text(
                                    &mut fs, 
                                    &new_text, 
                                    &Attrs::new(), 
                                    Shaping::Advanced
                                );
                                buffer_lock.shape_until_scroll(&mut fs, true);
                            }
                        }
                        
                        *cursor_x_clone.lock().unwrap() = cursor_x;
                        *cursor_y_clone.lock().unwrap() = cursor_y;
                        *dirty_clone.lock().unwrap() = true;
                        performer.grid.dirty = false;
                    }
                }
                Err(e) => {
                    eprintln!("PTY read error: {}", e);
                    break;
                }
            }
        }
        println!("PTY reader thread exiting");
    });

    println!("Returning PTY writer and child reference");
    Ok((writer_arc, child_ref))
}
}